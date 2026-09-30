defmodule Redoubt.Commandlet do
  @moduledoc ~S'''
  A commandlet: one command of the shell, declared once, with `defcommand`.

  The declaration types the command's parameters and carries its help: a one-line summary, a
  page, a line for each parameter, and examples. From it come the function the prompt calls, the
  checking of that function's arguments, the command's page in `help`, and in time completion
  (docs/userland/shell.md, "Completion").

      defmodule Redoubt.Shell.Helpers do
        use Redoubt.Commandlet, area: "Files"

        @summary "Copy a file"
        @help """
        Copies src to dst. Into an existing directory, the copy keeps its name.
        """
        @args src: "the file to copy", dst: "where the copy goes"
        @examples [{~S'cp("a.txt", "b.txt")', "copy a.txt to b.txt"}]
        defcommand cp(src :: path, dst :: path) do
          File.cp!(src, dst)
        end
      end

  - **Parameters** are `name :: type`, or `name :: type \\ default`; the types are
    `Redoubt.Commandlet.Param`'s. Parameters with a default come after those without, and a
    `many(type)` is the last.
  - **The arguments are checked before the body runs**, and coerced where a type says so (a
    `many(path)` given one path holds a list of one). A wrong one raises
    `Redoubt.Commandlet.UsageError`, which names the parameter and shows the usage.
  - **The help is not optional.** A command without a summary, a page, a line for each of its
    parameters, or an example that calls it does not compile. The help also becomes the
    function's `@doc`.
  - **Nothing else to wire.** `Redoubt.Commandlet.Registry` finds every module of the shell's
    application that uses `Redoubt.Commandlet`: their commands are imported at the prompt and
    listed by `help`, grouped by the `area` the module names.
  '''

  alias Redoubt.Commandlet.{Param, UsageError}

  defstruct [:name, :module, :area, :summary, :help, :params, :examples]

  @type t :: %__MODULE__{
          name: atom(),
          module: module(),
          area: String.t(),
          summary: String.t(),
          help: String.t(),
          params: [Param.t()],
          examples: [{String.t(), String.t()}]
        }

  defmacro __using__(opts) do
    area = Keyword.fetch!(opts, :area)

    quote do
      import Redoubt.Commandlet, only: [defcommand: 2]
      Module.register_attribute(__MODULE__, :commandlets, accumulate: true)
      @commandlet_area unquote(area)
      @before_compile Redoubt.Commandlet
    end
  end

  defmacro __before_compile__(env) do
    commandlets = env.module |> Module.get_attribute(:commandlets) |> Enum.reverse()

    quote do
      @doc false
      def __commandlets__, do: unquote(Macro.escape(commandlets))
    end
  end

  @doc """
  Declares a commandlet: `defcommand name(param :: type, ...) do ... end`, below its help
  (`@summary`, `@help`, `@args` and `@examples`).
  """
  defmacro defcommand(call, do: body) do
    {name, params} = signature!(call, __CALLER__)

    heads =
      Enum.map(params, fn p -> if p.default == :none, do: p.var, else: {:\\, [], [p.var, p.default]} end)

    vars = Enum.map(params, & &1.var)
    params = Enum.map(params, &to_param/1)

    quote do
      @commandlet Redoubt.Commandlet.__declare__(__ENV__, unquote(name), unquote(Macro.escape(params)))
      @commandlets @commandlet
      @doc Redoubt.Commandlet.__doc__(@commandlet)
      def unquote(name)(unquote_splicing(heads)) do
        unquote(vars) = Redoubt.Commandlet.check!(@commandlet, unquote(vars))
        unquote(body)
      end
    end
  end

  # The signature, read at expansion: every parameter `name :: type` or `name :: type \\ default`.
  defp signature!({name, _meta, args}, caller) when is_atom(name) and is_list(args) do
    # Flags are options: leaving them all out is `[]`, whether or not the signature says so.
    params =
      for param <- Enum.map(args, &param!(&1, name, caller)) do
        if match?({:flags, _}, param.type) and param.default == :none, do: %{param | default: []}, else: param
      end

    names = Enum.flat_map(params, &documented/1)
    defaulted = Enum.drop_while(params, &(&1.default == :none))
    positional = Enum.reject(params, &match?({:flags, _}, &1.type))

    cond do
      names != Enum.uniq(names) ->
        compile_error!(caller, "#{name}: two parameters or flags have one name")

      # Before the rule on defaults: flags always have one, and misplaced they would break it first.
      Enum.any?(Enum.drop(params, -1), &match?({:flags, _}, &1.type)) ->
        compile_error!(caller, "#{name}: only the last parameter may be flags(...)")

      Enum.any?(defaulted, &(&1.default == :none)) ->
        compile_error!(caller, "#{name}: parameters with a default come after those without")

      Enum.any?(Enum.drop(positional, -1), &match?({:many, _}, &1.type)) ->
        compile_error!(caller, "#{name}: only the last parameter, or the one before flags, may be many(...)")

      Enum.any?(Enum.drop(params, 1), &match?({:lines, _}, &1.type)) ->
        compile_error!(caller, "#{name}: only the first parameter may be lines, where |> puts them")

      true ->
        {name, params}
    end
  end

  defp signature!(_call, caller),
    do: compile_error!(caller, "write a command as `defcommand name(param :: type, ...) do ... end`")

  defp param!({:\\, _meta, [param, default]}, command, caller),
    do: %{param!(param, command, caller) | default: default}

  defp param!({:"::", _meta, [{name, _var_meta, context} = var, type]}, command, caller)
       when is_atom(name) and is_atom(context) do
    case Param.type(type) do
      {:ok, type} -> %{name: name, var: var, type: type, default: :none}
      {:error, message} -> compile_error!(caller, "#{command}: #{name}: #{message}")
    end
  end

  defp param!(other, command, caller),
    do: compile_error!(caller, "#{command}: write #{Macro.to_string(other)} as `name :: type`")

  # The names a declaration's @args must describe: a parameter's, or each of its flags'.
  defp documented(%{type: {:flags, flags}}), do: Enum.map(flags, & &1.name)
  defp documented(%{name: name}), do: [name]

  defp to_param(%{name: name, type: type, default: :none}), do: %Param{name: name, type: type}

  defp to_param(%{name: name, type: type, default: default}),
    do: %Param{name: name, type: type, default: {:default, Macro.to_string(default)}}

  defp compile_error!(env, message),
    do: raise(CompileError, file: env.file, line: env.line, description: "defcommand " <> message)

  @doc false
  # Takes the help written above a declaration, checks it, and makes the commandlet. It runs as the
  # module's body runs, which is when attributes such as @summary have values.
  def __declare__(env, name, params) do
    module = env.module
    [summary, help, args, examples] = for key <- [:summary, :help, :args, :examples], do: take(module, key)
    fail = &compile_error!(env, "#{name}: " <> &1)

    unless is_binary(summary) and String.trim(summary) != "" and not String.contains?(summary, "\n"),
      do: fail.("no @summary: one line saying what it does")

    unless is_binary(help) and String.trim(help) != "",
      do: fail.("no @help: a page saying what it does and how")

    args = args || []
    names = Enum.flat_map(params, &documented/1)

    unless Keyword.keyword?(args) and Enum.all?(args, fn {_name, doc} -> is_binary(doc) end),
      do: fail.("@args is a keyword list of a line for each parameter and flag")

    for missing <- names -- Keyword.keys(args), do: fail.("no @args line for #{missing}")

    for extra <- Keyword.keys(args) -- names,
        do: fail.("@args names #{extra}, which is not a parameter or a flag")

    unless is_list(examples) and examples != [],
      do: fail.("no @examples: a list of {code, what it does}, the code calling #{name}")

    for example <- examples, do: check_example(example, name, fail)

    %__MODULE__{
      name: name,
      module: module,
      area: Module.get_attribute(module, :commandlet_area),
      summary: String.trim(summary),
      help: String.trim(help),
      params: Enum.map(params, &with_doc(&1, args)),
      examples: examples
    }
  end

  defp with_doc(%Param{type: {:flags, flags}} = param, args),
    do: %{param | type: {:flags, Enum.map(flags, &with_doc(&1, args))}}

  defp with_doc(%Param{name: name} = param, args), do: %{param | doc: args[name]}

  # A parameter's rows in help: its own, or one for each of its flags, written as they are given.
  defp rows(%Param{type: {:flags, flags}}), do: for(f <- flags, do: {"#{f.name}:", Param.name(f.type), f.doc})
  defp rows(%Param{} = p), do: [{Atom.to_string(p.name), Param.name(p.type), p.doc}]

  defp take(module, key) do
    value = Module.get_attribute(module, key)
    Module.delete_attribute(module, key)
    value
  end

  defp check_example({code, what}, name, fail) when is_binary(code) and is_binary(what) do
    case Code.string_to_quoted(code) do
      {:ok, quoted} -> calls?(quoted, name) or fail.("the example #{inspect(code)} does not call #{name}")
      {:error, _reason} -> fail.("the example #{inspect(code)} is not Elixir")
    end
  end

  defp check_example(example, _name, fail),
    do: fail.("an example is {code, what it does}, both strings, not #{inspect(example)}")

  defp calls?(quoted, name) do
    {_quoted, found} =
      Macro.prewalk(quoted, false, fn
        {^name, _meta, args} = node, _found when is_list(args) -> {node, true}
        node, found -> {node, found}
      end)

    found
  end

  @doc false
  def __doc__(%__MODULE__{} = cmd) do
    params =
      for {name, type, doc} <- Enum.flat_map(cmd.params, &rows/1), do: "  * `#{name}` (#{type}): #{doc}"

    examples = for {code, what} <- cmd.examples, do: "    #{code}    # #{what}"
    sections = [cmd.summary <> ".", cmd.help, "## Parameters\n\n" <> Enum.join(params, "\n")]
    sections = if params == [], do: List.delete_at(sections, 2), else: sections
    Enum.join(sections ++ ["## Examples\n\n" <> Enum.join(examples, "\n")], "\n\n") <> "\n"
  end

  @doc """
  Checks the arguments given to `cmd` against its parameters, and returns them as the body gets
  them, coerced where a type says so; raises `Redoubt.Commandlet.UsageError` at the first that
  does not fit.
  """
  @spec check!(t(), [term()]) :: [term()]
  def check!(%__MODULE__{} = cmd, values) do
    cmd.params
    |> Enum.zip(values)
    |> Enum.map(fn {param, value} ->
      case Param.check(param, value) do
        {:ok, value} -> value
        {:error, hint} -> raise UsageError, message: refusal(cmd, param, value, hint), command: cmd.name
      end
    end)
  end

  defp refusal(cmd, _param, _value, {:message, message}), do: "#{cmd.name}: #{message}\n" <> usage_line(cmd)

  defp refusal(cmd, param, value, hint) do
    got = inspect(value, limit: 5, printable_limit: 60)
    hint = if hint, do: "; #{hint}", else: ""
    "#{cmd.name}: #{param.name} must be #{Param.describe(param.type)}, got #{got}#{hint}\n" <> usage_line(cmd)
  end

  defp usage_line(cmd), do: "usage: " <> usage(cmd)

  @doc "How `cmd` is called: `head(lines, count \\\\ 10)`."
  @spec usage(t()) :: String.t()
  def usage(%__MODULE__{} = cmd), do: "#{cmd.name}(#{Enum.map_join(cmd.params, ", ", &Param.usage/1)})"

  @doc "The name and arity of each function `cmd` defines, for importing: one for each default left out."
  @spec arities(t()) :: [{atom(), arity()}]
  def arities(%__MODULE__{name: name, params: params}) do
    required = Enum.count(params, &(&1.default == :none))
    for arity <- required..length(params)//1, do: {name, arity}
  end

  @doc "Every command's name and summary, one line each, grouped by area: what `help()` shows."
  @spec index([t()]) :: [String.t()]
  def index(commandlets) do
    width = commandlets |> Enum.map(&String.length(Atom.to_string(&1.name))) |> Enum.max(fn -> 0 end)

    areas =
      commandlets
      |> Enum.group_by(& &1.area)
      |> Enum.sort_by(fn {area, _commandlets} -> area end)
      |> Enum.map(fn {area, cmds} ->
        [area | for(cmd <- cmds, do: "  #{pad(cmd.name, width)}  #{cmd.summary}")]
      end)

    List.flatten(["Commands, by area. help(:name) shows a command's page.", "", Enum.intersperse(areas, "")])
  end

  @doc "A command's page: its usage, summary, parameters, help and examples."
  @spec page(t()) :: [String.t()]
  def page(%__MODULE__{} = cmd) do
    rows = Enum.flat_map(cmd.params, &rows/1)
    name_width = rows |> Enum.map(&String.length(elem(&1, 0))) |> Enum.max(fn -> 0 end)
    type_width = rows |> Enum.map(&String.length(elem(&1, 1))) |> Enum.max(fn -> 0 end)
    code_width = cmd.examples |> Enum.map(&String.length(elem(&1, 0))) |> Enum.max()

    params =
      for {name, type, doc} <- rows, do: "  #{pad(name, name_width)}  #{pad(type, type_width)}  #{doc}"

    examples = for {code, what} <- cmd.examples, do: "  #{pad(code, code_width)}  #{what}"
    params = if params == [], do: [], else: params ++ [""]

    [usage(cmd), "", cmd.summary, ""] ++
      params ++ String.split(cmd.help, "\n") ++ ["", "Examples"] ++ examples
  end

  defp pad(text, width), do: text |> to_string() |> String.pad_trailing(width)
end
