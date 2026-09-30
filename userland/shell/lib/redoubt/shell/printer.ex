defmodule Redoubt.Shell.Printer do
  @moduledoc """
  Prints values, errors and messages at the prompt, every line through `Redoubt.Term.Text`.

  A `%Lines{}` is printed a line at a time, as it is read; any other value is inspected, which
  never reads a `%Lines{}`. An error's stack trace stops where the shell's own evaluation
  begins.
  """

  alias Redoubt.Commandlet
  alias Redoubt.Commandlet.{Registry, UsageError}
  alias Redoubt.Term.Text
  alias Redoubt.Util.Lines

  # Lines written to the console in one request.
  @batch 64

  # The frames of the evaluation machinery under every line, left out of a stack trace.
  @machinery [Redoubt.Shell.Evaluator, Code, :elixir, :elixir_eval, :elixir_expand, :erl_eval]

  @doc "Prints a value."
  def value(%Lines{} = lines) do
    lines
    |> Stream.chunk_every(@batch)
    |> Enum.each(fn batch -> IO.write(Enum.map(batch, &[Text.visible(&1), ?\n])) end)
  end

  def value(value), do: value |> inspect(pretty: true, width: width()) |> text()

  @doc """
  Prints an error caught with `kind` and `reason`, and the part of its stack that is the line's.
  A command's usage error is printed alone: it says what was wrong and how to call the command,
  and the stack would only show the checking.
  """
  def error(:error, %UsageError{} = error, _stack), do: text(Exception.format_banner(:error, error))

  def error(kind, reason, stack) do
    frames = Enum.take_while(stack, fn {module, _fun, _arity, _location} -> module not in @machinery end)
    trace = if frames == [], do: "", else: "\n" <> String.trim_trailing(Exception.format_stacktrace(frames))
    text(Exception.format_banner(kind, reason, stack) <> trace)
  end

  @doc """
  Prints one of the compiler's diagnostics. An undefined variable that is a command's name, as a
  bare `pwd` is, is a call written without its parentheses, and the message says so.
  """
  def diagnostic(%{severity: severity, message: message} = diagnostic) do
    text("#{severity}: #{message}#{location(diagnostic[:position])}#{hint(message)}")
  end

  defp location(line) when is_integer(line) and line > 0, do: " (shell:#{line})"
  defp location({line, column}), do: " (shell:#{line}:#{column})"
  defp location(_none), do: ""

  defp hint(message) do
    with [_all, name] <- Regex.run(~r/^undefined variable "(\w+)"$/, message),
         {:ok, cmd} <- Registry.fetch(name),
         true <- {cmd.name, 0} in Commandlet.arities(cmd) do
      "\n  #{name} is a command: call it as #{name}()"
    else
      _other -> ""
    end
  end

  @doc "Prints text, a line at a time."
  def text(text), do: text |> String.split("\n") |> Enum.map(&[Text.visible(&1), ?\n]) |> IO.write()

  defp width do
    case :io.columns() do
      {:ok, columns} -> columns
      {:error, _reason} -> 80
    end
  end
end
