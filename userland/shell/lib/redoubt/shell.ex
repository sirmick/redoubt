defmodule Redoubt.Shell do
  @moduledoc """
  Redoubt's shell: a read-eval-print loop over Elixir, in the session's VM. It is not IEx.

  It reads a line, reading more while the input is unfinished, parses it, and evaluates it in a
  fresh evaluator process, which also prints the result (`Redoubt.Shell.Evaluator`). This
  process keeps the bindings and the environment between lines, so an evaluation that dies loses
  only its own line. Everything it prints goes through `Redoubt.Term.Text`, so no byte of a
  value, a file or a message reaches the terminal as a control sequence.

  At the prompt, every commandlet is imported (`Redoubt.Commandlet`), and `help()` lists them.
  `exit` or the end of the input ends the shell. Nothing is read or run at start but the shell
  itself. A session starts in its home, the directory the steward told it of (`enter_home/1`);
  a VM that is no session starts where it is.

  Lines are read from the group leader with `IO.gets/1`. On a console that is OTP's `group`,
  under the shell's own driver (`Redoubt.Shell.Driver`): the line is edited by `edlin`, with
  history, and drawn by `Redoubt.Term`, so everything the console shows, what a line writes to
  it itself with `IO.puts/1` included, passes the same guard. The interrupt, Ctrl+C or Ctrl+\\,
  ends the line being read, or the line being evaluated, with the pipelines it runs, and never the
  shell. Tab completes (`Redoubt.Shell.Completer`), from the
  prompt's names as they are at each read.

  In the **machine mode** (docs/userland/shell.md, "The machine mode"), entered by `machine/1`
  with a token, the lines come from the driver, one for each asked for
  (`{:redoubt_machine, :next, self()}`, then `{:redoubt_machine, :line, text}`), with no prompt,
  echo or editing, and a line that is exactly the token ends a block, parsed and evaluated whole.
  Every block's end is marked with one line, `token n ok|error|exit`, `n` the counter the prompt
  would show, drawn by the driver for this process at the start of a line, through the guard
  like everything else. The token, and the key the driver minted for this process at its start
  (`Redoubt.Shell.Driver.machine_key/1`), which every ask and mark carries, are held in this
  loop's state alone: never in the bindings or the environment the evaluated code gets, and the
  line that set the token is cut from `group`'s history; and since a line is asked for only
  between blocks, no later line is in this process's mailbox while a block runs.
  """

  alias Redoubt.Commandlet.Registry
  alias Redoubt.Shell.{Completer, Driver, Evaluator, Notices, Printer}
  alias Redoubt.Term.Text

  # beamlet's natives (docs/userland/beamlet.md, "Natives"): no module, so nothing to check at compile time.
  @compile {:no_warn_undefined, :redoubt}

  @doc """
  beamlet's entry point (`beamlet ... Elixir.Redoubt.Shell`): starts the shell's application,
  and so `:elixir`, whose tables compiling a module at the prompt needs, then runs the driver
  with the shell on it until the shell ends.
  """
  def start do
    {:ok, _started} = Application.ensure_all_started(:redoubt_shell)
    # The steward's lease notices, served for the VM's life (`Redoubt.Shell.Notices`); a VM
    # launched with no `notices` handle has none to serve.
    _ = Notices.start()
    Driver.run()
  end

  @doc "Starts the shell in a process of its own, as `group` asks of its shell, and returns its pid."
  @spec start_link(keyword()) :: pid()
  def start_link(opts \\ []), do: spawn_link(fn -> run(opts) end)

  @doc """
  Runs the shell on the group leader until `exit` or the end of the input, and returns `:ok`.

  Options:
  - `:banner` (`true`): print the greeting first.
  - `:max_heap_words`: the evaluator's heap limit, in words (`Redoubt.Shell.Evaluator`).
  """
  @spec run(keyword()) :: :ok
  def run(opts \\ []) do
    if Keyword.get(opts, :banner, true) do
      Printer.text("Redoubt shell, on Elixir #{System.version()}. `exit` or Ctrl+D ends it.")
    end

    enter_home(identity())

    # The driver the interrupt comes from, if the shell runs on its console; and the machine
    # mode's key, asked for now, before any line runs, so no line can be the first to ask.
    driver = Driver.of_group()
    limits = Keyword.take(opts, [:max_heap_words]) ++ [driver: driver]

    state = %{
      counter: 1,
      binding: [],
      env: prompt_env(),
      limits: limits,
      machine: nil,
      key: Driver.machine_key(driver)
    }

    loop(state)
  end

  @doc """
  Starts the session in its home, the directory `identity` (what the steward told the session,
  `redoubt:identity/0`) names: the working directory moves there, and the prompt shows it. A
  session told no home, or whose home cannot be entered, says so once and starts where the VM
  did, `/`, where nothing is writable; a labelled session told no home is one in a confined
  boot, whose home slot the steward left unbound, and its line says that a push is the way in
  (docs/servers/steward.md, "Declassification and push"). A VM that is no session (`nil`, on a
  host) starts where it is, and nothing is said.

  Returns `{:ok, home}`, `{:error, reason}` for a session that could not start in its home, or
  `:none` for a VM that is no session.
  """
  @spec enter_home(map() | nil) :: {:ok, String.t()} | {:error, term()} | :none
  def enter_home(nil), do: :none

  def enter_home(identity) do
    result =
      case identity do
        %{home: home} when is_binary(home) -> with(:ok <- File.cd(home), do: {:ok, home})
        _no_home -> {:error, :no_home}
      end

    with {:error, reason} <- result do
      here =
        case File.cwd() do
          {:ok, dir} -> Text.visible(dir)
          {:error, _reason} -> "?"
        end

      case {reason, Map.get(identity, :labels, [])} do
        {:no_home, [_ | _]} ->
          Printer.text("** no home in this label set: a push is the way in; this session starts in #{here}")

        _ ->
          Printer.text("** no home to start in (#{inspect(reason)}); this session starts in #{here}")
      end

      result
    end
  end

  # What the steward told the session of itself, or nil for a VM that is no session, or has no
  # beamlet natives. Read here, not through `Redoubt.Shell.Session`, so the start loads no
  # command's module: the prompt's footprint is measured.
  defp identity do
    case :redoubt.identity() do
      {:ok, identity} -> identity
      {:error, _} -> nil
    end
  rescue
    UndefinedFunctionError -> nil
  end

  defp loop(state) do
    case if(state.machine, do: read_block(state, [], nil), else: read(state, "")) do
      :done ->
        _ = ended(state, :exit)
        :ok

      :blank ->
        loop(state)

      # An empty block: nothing to evaluate, and its mark says so.
      :empty ->
        loop(ended(state, :ok))

      {:error, message} ->
        Printer.text(message)
        loop(ended(state, :error))

      {:ok, quoted} ->
        case Evaluator.eval(quoted, state.binding, state.env, state.limits) do
          {:ok, binding, env} ->
            loop(ended(%{state | binding: binding, env: env}, :ok))

          :error ->
            loop(ended(state, :error))

          :exit ->
            _ = ended(state, :exit)
            :ok

          # Leaving the mode, the block is marked under the token it was sent with; entering,
          # or changing the token, the line is marked under the token it set.
          {:machine, nil} ->
            loop(state |> ended(:ok) |> leave_machine())

          {:machine, token} ->
            loop(state |> enter_machine(token) |> ended(:ok))
        end
    end
  end

  # A line has ended: in the machine mode its mark is drawn, and the counter moves on.
  defp ended(%{machine: nil} = state, _status), do: %{state | counter: state.counter + 1}

  defp ended(%{machine: token} = state, status) do
    send(driver(state), {:redoubt_machine, :mark, state.key, "#{token} #{state.counter} #{status}"})

    receive do
      {:redoubt_machine, :marked} -> :ok
    end

    %{state | counter: state.counter + 1}
  end

  # ---- the machine mode ----

  defp driver(%{limits: limits}), do: Keyword.fetch!(limits, :driver)

  # The driver takes the keys for the mode; `machine/1` has checked there is one. A screen or a
  # feed a line left behind holds them instead, and then the mode is refused and the prompt stays;
  # so is a shell the driver gave no key (another process asked first, which no line can).
  defp enter_machine(%{machine: nil, key: nil} = state, _token) do
    Printer.text("** machine: the terminal gave this shell no key, so the mode cannot open")
    state
  end

  defp enter_machine(%{machine: nil} = state, token) do
    send(driver(state), {:redoubt_machine, :open, state.key})

    receive do
      {:redoubt_machine, :opened} ->
        %{state | machine: token}

      {:redoubt_machine, :refused} ->
        Printer.text("** machine: a screen or a feed holds the terminal, so the mode cannot open")
        state
    end
  end

  defp enter_machine(state, token), do: %{state | machine: token}

  defp leave_machine(%{machine: nil} = state), do: state

  defp leave_machine(state) do
    send(driver(state), {:redoubt_machine, :close, state.key})

    receive do
      {:redoubt_machine, :closed} -> :ok
    end

    flush_machine()
    %{state | machine: nil}
  end

  # Lines the driver sent before it was told to close are no block's.
  defp flush_machine do
    receive do
      {:redoubt_machine, _what} -> flush_machine()
      {:redoubt_machine, _what, _detail} -> flush_machine()
    after
      0 -> :ok
    end
  end

  # A block: the lines up to the one that is exactly the token, each asked for in turn, so none
  # waits in this mailbox while a block runs. A line the driver dropped for its length (`long`,
  # the bound) fails the block; the interrupt drops the lines read so far; the end of the input
  # ends the shell.
  defp read_block(%{machine: token} = state, lines, long) do
    send(driver(state), {:redoubt_machine, :next, state.key})

    receive do
      {:redoubt_machine, :line, ^token} -> block(state, lines, long)
      {:redoubt_machine, :line, line} -> read_block(state, [line | lines], long)
      {:redoubt_machine, :long, bound} -> read_block(state, lines, bound)
      {:redoubt_machine, :interrupt} -> read_block(state, [], nil)
      {:redoubt_machine, :eof} -> :done
    end
  end

  # A block is parsed whole: one that ends before its expression does is wrong, not unfinished,
  # since the token said where it ends.
  defp block(_state, _lines, bound) when is_integer(bound),
    do:
      {:error, "** shell: a line of the block was longer than #{bound} bytes, so the block was not evaluated"}

  defp block(state, lines, nil) do
    code = lines |> Enum.reverse() |> Enum.join("\n")

    case String.trim(code) do
      "" ->
        :empty

      "exit" ->
        :done

      _code ->
        case parse(code, state) do
          :incomplete -> {:error, "** (SyntaxError) shell: the block ended before its expression did"}
          result -> result
        end
    end
  end

  defp read(state, sofar) do
    # Tab completes from what the prompt has now; on a console that is not OTP's group, the
    # option is refused and there is no completion.
    # The completer's module is loaded at the first Tab, not at the prompt: the closure holds the
    # prompt's names alone.
    vars = for {name, _value} <- state.binding, is_atom(name), do: name
    {imports, aliases} = {state.env.functions ++ state.env.macros, state.env.aliases}
    _ = :io.setopts(expand_fun: fn before -> Completer.tab(before, vars, imports, aliases) end)

    case IO.gets(prompt(state, sofar)) do
      :eof ->
        :done

      # The driver's end of input, Ctrl+D on an empty line or the console's end, which group
      # can carry only as an error to the read (`Redoubt.Shell.Driver`).
      {:error, :eof} ->
        :done

      # The interrupt: the line is dropped, whatever of it was read, and the next is read.
      {:error, :interrupted} ->
        :blank

      {:error, reason} ->
        Printer.text("** the console could not be read (#{inspect(reason)}), so the shell ends")
        :done

      data ->
        code = sofar <> IO.chardata_to_string(data)

        # A byte that is not UTF-8 (a pasted Latin-1 character, an 8-bit Meta key) makes the line
        # wrong, not the shell: it is shown, and the next line is read.
        if String.valid?(code) do
          case classify(sofar, code, state) do
            :incomplete -> read(state, code)
            result -> result
          end
        else
          {:error,
           "** (SyntaxError) shell: the line holds bytes that are not UTF-8: #{String.trim_trailing(code)}"}
        end
    end
  end

  # A first line may be blank or `exit`; a continuation line is always more of the expression.
  defp classify("", code, state) do
    case String.trim(code) do
      "" -> :blank
      "exit" -> :done
      _code -> parse(code, state)
    end
  end

  defp classify(_sofar, code, state), do: parse(code, state)

  # The tokenizer's warnings are taken, as the compiler's are, and printed through the printer;
  # and whatever it raises ends this line, never the shell.
  defp parse(code, state) do
    {result, diagnostics} =
      Code.with_diagnostics([log: false], fn ->
        try do
          Code.string_to_quoted(code, file: "shell", line: state.counter)
        rescue
          error -> {:raised, error}
        end
      end)

    Enum.each(diagnostics, &Printer.diagnostic/1)

    case result do
      {:raised, error} ->
        {:error, "** (SyntaxError) shell: " <> Exception.message(error)}

      {:ok, quoted} ->
        {:ok, quoted}

      # An error with no token is the input ending too soon: `1 +`, `defmodule M do`, an
      # unclosed string. It is not finished, so it is not wrong yet.
      {:error, {_meta, _message, ""}} ->
        :incomplete

      {:error, {meta, message, token}} ->
        {:error, "** (SyntaxError) shell:#{meta[:line]}:#{meta[:column]}: #{message(message, token)}"}
    end
  end

  defp message({prefix, suffix}, token), do: prefix <> token <> suffix
  defp message(prefix, token), do: prefix <> token

  defp prompt(state, "") do
    dir =
      case File.cwd() do
        {:ok, dir} -> Text.visible(dir)
        {:error, _reason} -> "?"
      end

    "#{dir} (#{state.counter})> "
  end

  defp prompt(state, _sofar), do: "...(#{state.counter})> "

  # The environment every line is evaluated in: every commandlet imported, as a user's own
  # `import`, `alias` or `require` at the prompt then adds to it.
  # Every commandlet imported, and `Job` (`Redoubt.Job`), as the pages write it: each its own form,
  # since a form of several would be evaluated by erl_eval, which nothing else loads at the prompt.
  defp prompt_env do
    [Registry.imports(), quote(do: alias(Redoubt.Job))]
    |> Enum.reduce(Code.env_for_eval(file: "shell"), fn form, env ->
      {_value, _binding, env} = Code.eval_quoted_with_env(form, [], env)
      env
    end)
  end
end
