defmodule Redoubt.Shell.Completer do
  @moduledoc """
  Completion at the prompt (docs/userland/shell.md, "Completion"): what Tab offers for the line
  before the cursor. It is `group`'s `expand_fun`, which `group` calls in its own process; the
  shell sets it before each read, a closure over the names the prompt then has calling `tab/4`,
  so this module is loaded at the first Tab and not before.

  What is completed, by where the cursor is:
  - a name being typed: the commands and functions imported at the prompt, and its variables;
  - `Mod.fu`, `:mod.fu`: the module's exported functions;
  - `Mo`: aliases and modules, a segment at a time;
  - inside a string, or after `:`, given to a command: what that parameter is declared as; a
    `path` completes from its directory, a `command` or `name` from the commands and help topics.

  The first Tab inserts what every candidate shares; with nothing more to insert, `group` lists
  them below the line. A directory completes with a trailing `/`. Listing a directory or the
  modules is one read, done with the session's own authority, as `ls` would, and given 300 ms:
  past that, Tab inserts and lists nothing. A completer never writes and never starts a program.
  """

  alias Redoubt.Commandlet.Registry
  alias Redoubt.Shell.Topics

  @timeout 300

  @type context :: %{vars: [String.t()], imports: [String.t()], aliases: %{String.t() => module()}}

  @doc """
  What Tab does, as `group` asks it: `before` is the line before the cursor, reversed, as `group`
  keeps it; `vars`, `imports` and `aliases` are the prompt's variables' names, its
  `env.functions ++ env.macros` and its `env.aliases`. The context is made here, at the Tab.
  """
  @spec tab(charlist(), [atom()], [{module(), [{atom(), arity()}]}], [{module(), module()}]) ::
          {:yes | :no, charlist(), [charlist()]}
  def tab(before, vars, imports, aliases),
    do: before |> Enum.reverse() |> List.to_string() |> expand(context(vars, imports, aliases))

  @doc "The context of a prompt: its variables' names, its imports' names and its aliases."
  @spec context(Code.binding(), Macro.Env.t()) :: context()
  def context(binding, env) do
    vars = for {name, _value} <- binding, is_atom(name), do: name
    context(vars, env.functions ++ env.macros, env.aliases)
  end

  defp context(vars, imports, aliases) do
    imports = for {_module, names} <- imports, {name, _arity} <- names, do: Atom.to_string(name)

    %{
      vars: Enum.map(vars, &Atom.to_string/1),
      imports: Enum.filter(imports, &Regex.match?(~r/^[a-z]\w*[?!]?$/, &1)),
      aliases: Map.new(aliases, fn {as, module} -> {inspect(as), module} end)
    }
  end

  @doc """
  What Tab does after `before`, the line up to the cursor: `{:yes, insert, candidates}`, or
  `{:no, [], []}` when nothing completes it.
  """
  @spec expand(String.t(), context()) :: {:yes | :no, charlist(), [charlist()]}
  def expand(before, context) do
    case scan(before, :code, "") do
      {head, nil} -> code(before, head, context)
      {head, typed} -> by_parameter(head, typed, :string)
    end
  end

  defp code(before, head, context) do
    case Code.Fragment.cursor_context(before) do
      {:local_or_var, hint} ->
        hint = List.to_string(hint)
        functions = Enum.map(context.imports, &{&1, "("})
        offer(hint, Enum.map(context.vars, &{&1, ""}) ++ functions)

      {:dot, {:alias, alias}, hint} ->
        functions(module(List.to_string(alias), context), List.to_string(hint))

      {:dot, {:unquoted_atom, module}, hint} ->
        functions(existing(List.to_string(module)), List.to_string(hint))

      {:alias, hint} ->
        hint = List.to_string(hint)
        bounded(fn -> modules(hint, context) end)

      {:unquoted_atom, hint} ->
        hint = List.to_string(hint)
        by_parameter(String.replace_suffix(head, ":" <> hint, ""), hint, :atom)

      _other ->
        no()
    end
  end

  # ---- an argument given to a command ----

  # The line with the contents of each closed string and charlist taken out, and what is typed
  # in one left open at its end (nil when there is none): `cp("a b", "no` is
  # `{~S|cp("", |, "no"}`. Escapes are skipped; `?(` and `?"` are characters, not brackets or
  # quotes, but `exists?(` is a call.
  defp scan(<<>>, :code, head), do: {head, nil}
  defp scan(<<>>, {:string, _quote, typed}, head), do: {head, typed}

  defp scan(<<??, rest::binary>>, :code, head) do
    case {Regex.match?(~r/[\p{L}\p{N}_]$/u, head), rest} do
      {false, <<_char::utf8, rest::binary>>} -> scan(rest, :code, head <> "?_")
      _name_or_end -> scan(rest, :code, head <> "?")
    end
  end

  defp scan(<<q, rest::binary>>, :code, head) when q in [?", ?'], do: scan(rest, {:string, q, ""}, head)
  defp scan(<<c, rest::binary>>, :code, head), do: scan(rest, :code, head <> <<c>>)

  defp scan(<<?\\, c, rest::binary>>, {:string, q, typed}, head),
    do: scan(rest, {:string, q, typed <> <<?\\, c>>}, head)

  defp scan(<<q, rest::binary>>, {:string, q, _typed}, head), do: scan(rest, :code, head <> <<q, q>>)

  defp scan(<<c, rest::binary>>, {:string, q, typed}, head),
    do: scan(rest, {:string, q, typed <> <<c>>}, head)

  # What the parameter the cursor is an argument for completes from: a path only in a string.
  defp by_parameter(head, typed, form) do
    case {parameter(head), form} do
      {{:path, _opts}, :string} -> bounded(fn -> paths(typed) end)
      {{type, _opts}, _form} when type in [:command, :name] -> offer(typed, names())
      _other -> no()
    end
  end

  # The type of the command's parameter the cursor, at the end of `head`, is an argument for.
  # Nothing typed is parsed, so nothing typed becomes an atom: the call is found by its brackets
  # and commas (`call/1`), and its name is looked up as a string.
  defp parameter(head) do
    with {name, index} <- call(head),
         {:ok, command} <- Registry.fetch(name),
         %{type: type} <- Enum.at(command.params, index) || List.last(command.params) do
      case type do
        {:many, inner} -> inner
        type -> type
      end
    else
      _none -> nil
    end
  end

  # The call the end of `head` is inside, as the name before its innermost unclosed `(` and the
  # argument's place, its commas; a call piped into takes its first argument from the pipe. Not
  # inside a call's own brackets (a list, a tuple), or not a plain name (`Mod.fun(`, `ñcat(`):
  # nil.
  defp call(head) do
    with {at, commas} <- innermost(head, 0, []),
         before = binary_part(head, 0, at),
         [name] <- Regex.run(~r/[a-z_][a-zA-Z0-9_]*[?!]?$/, before),
         rest = binary_part(before, 0, byte_size(before) - byte_size(name)),
         false <- Regex.match?(~r/[\p{L}\p{N}_.:@]$/u, rest) do
      piped = rest |> String.trim_trailing() |> String.ends_with?("|>")
      {name, commas + if(piped, do: 1, else: 0)}
    else
      _none -> nil
    end
  end

  # The open brackets, innermost first, each `{bracket, at, commas}`; at the end, the innermost if
  # it is a `(`.
  defp innermost(head, at, open) when at == byte_size(head) do
    case open do
      [{?(, paren, commas} | _] -> {paren, commas}
      _none -> nil
    end
  end

  defp innermost(head, at, open) do
    case {:binary.at(head, at), open} do
      {b, _} when b in [?(, ?[, ?{] -> innermost(head, at + 1, [{b, at, 0} | open])
      {b, [_ | outer]} when b in [?), ?], ?}] -> innermost(head, at + 1, outer)
      {?,, [{b, paren, commas} | outer]} -> innermost(head, at + 1, [{b, paren, commas + 1} | outer])
      _other -> innermost(head, at + 1, open)
    end
  end

  # ---- the sources ----

  defp names do
    commands = for name <- Registry.names(), do: {name, ""}
    topics = for {name, _title} <- Topics.all(), do: {name, ""}
    commands ++ topics
  end

  # A module named as typed, through an alias; nil when no such atom exists.
  defp module(alias, context) do
    [first | rest] = String.split(alias, ".")

    case Map.fetch(context.aliases, first) do
      {:ok, module} -> existing(Enum.join([Atom.to_string(module) | rest], "."))
      :error -> existing("Elixir." <> alias)
    end
  end

  # The atom `name` is, if it is one already: a name typed is never made an atom.
  defp existing(name) do
    String.to_existing_atom(name)
  rescue
    ArgumentError -> nil
  end

  defp functions(module, hint) do
    if module != nil and Code.ensure_loaded?(module) do
      exports = for {name, _arity} <- module.module_info(:exports), do: {Atom.to_string(name), "("}
      offer(hint, Enum.reject(exports, fn {name, _} -> name in ["module_info", "__info__"] end))
    else
      no()
    end
  end

  # Modules and aliases a segment at a time: `Fi` offers `File`, and `File.` then `File.Stat`.
  defp modules(hint, context) do
    names = for {~c"Elixir." ++ name, _file, _loaded} <- :code.all_available(), do: List.to_string(name)

    known = Map.keys(context.aliases) ++ names
    dot = String.length(hint) - String.length(List.last(String.split(hint, ".")))

    segments =
      for name <- known, String.starts_with?(name, hint) do
        rest = String.slice(name, dot..-1//1)
        {String.slice(name, 0, dot) <> hd(String.split(rest, ".")), ""}
      end

    offer(hint, segments)
  end

  defp paths(typed) do
    {dir, base} =
      case String.split(typed, "/") do
        [base] -> {"", base}
        parts -> {Enum.join(Enum.drop(parts, -1), "/") <> "/", List.last(parts)}
      end

    listed = if dir == "", do: ".", else: dir

    case File.ls(listed) do
      {:ok, entries} ->
        candidates =
          for entry <- entries,
              String.starts_with?(entry, base),
              base != "" or not String.starts_with?(entry, ".") do
            if File.dir?(Path.join(listed, entry)), do: {dir <> entry <> "/", ""}, else: {dir <> entry, ""}
          end

        offer(typed, candidates)

      {:error, _reason} ->
        no()
    end
  end

  # A slow source (a directory over a slow server, the modules of a long code path) gets
  # @timeout ms in a process of its own; past that, nothing is offered.
  defp bounded(fun) do
    {pid, ref} = spawn_monitor(fn -> exit({:completed, fun.()}) end)

    receive do
      {:DOWN, ^ref, :process, ^pid, {:completed, result}} -> result
      {:DOWN, ^ref, :process, ^pid, _crashed} -> no()
    after
      @timeout ->
        Process.exit(pid, :kill)
        Process.demonitor(ref, [:flush])
        no()
    end
  end

  # ---- what Tab does ----

  # `candidates` are `{text, suffix}`: the suffix goes after a candidate completed alone (`(`
  # after a function's name).
  defp offer(hint, candidates) do
    matches =
      candidates
      |> Enum.filter(fn {text, _suffix} -> String.starts_with?(text, hint) end)
      |> Enum.uniq_by(&elem(&1, 0))
      |> Enum.sort()

    case matches do
      [] ->
        no()

      [{text, suffix}] ->
        {:yes, String.to_charlist(String.replace_prefix(text, hint, "") <> suffix), []}

      many ->
        texts = Enum.map(many, &elem(&1, 0))

        {:yes, String.to_charlist(String.replace_prefix(common(texts), hint, "")),
         Enum.map(texts, &String.to_charlist/1)}
    end
  end

  defp common([first | rest]), do: Enum.reduce(rest, first, &prefix/2)

  # By graphemes, so never half of one.
  defp prefix(a, b) do
    String.graphemes(a)
    |> Enum.zip(String.graphemes(b))
    |> Enum.take_while(fn {x, y} -> x == y end)
    |> Enum.map_join(&elem(&1, 0))
  end

  defp no, do: {:no, [], []}
end
