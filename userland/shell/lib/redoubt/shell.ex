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
  itself.

  Lines are read from the group leader with `IO.gets/1`. On a console that is OTP's `group`,
  under the shell's own driver (`Redoubt.Shell.Driver`): the line is edited by `edlin`, with
  history, and drawn by `Redoubt.Term`, so everything the console shows, what a line writes to
  it itself with `IO.puts/1` included, passes the same guard. The interrupt, Ctrl+C or Ctrl+\\,
  ends the line being read, not the shell.
  """

  alias Redoubt.Commandlet.Registry
  alias Redoubt.Shell.{Driver, Evaluator, Printer}
  alias Redoubt.Term.Text

  @doc """
  beamlet's entry point (`beamlet ... Elixir.Redoubt.Shell`): starts the shell's application,
  and so `:elixir`, whose tables compiling a module at the prompt needs, then runs the driver
  with the shell on it until the shell ends.
  """
  def start do
    {:ok, _started} = Application.ensure_all_started(:redoubt_shell)
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

    limits = Keyword.take(opts, [:max_heap_words])
    loop(%{counter: 1, binding: [], env: prompt_env(), limits: limits})
  end

  defp loop(state) do
    case read(state, "") do
      :done ->
        :ok

      :blank ->
        loop(state)

      {:error, message} ->
        Printer.text(message)
        loop(%{state | counter: state.counter + 1})

      {:ok, quoted} ->
        case Evaluator.eval(quoted, state.binding, state.env, state.limits) do
          {:ok, binding, env} -> loop(%{state | binding: binding, env: env, counter: state.counter + 1})
          :error -> loop(%{state | counter: state.counter + 1})
          :exit -> :ok
        end
    end
  end

  defp read(state, sofar) do
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
  defp prompt_env do
    {_value, _binding, env} =
      Code.eval_quoted_with_env(Registry.imports(), [], Code.env_for_eval(file: "shell"))

    env
  end
end
