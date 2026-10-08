defmodule Redoubt.Util.Lines do
  @moduledoc """
  Lines of text, read as they are consumed.

  `cat/1` makes one over a file, and `grep/2` and `head/2` over another; nothing is read until
  something enumerates it, and a consumer that stops early stops the reading (a file is closed
  then). A line carries no line ending. At the prompt, a `%Lines{}` is printed a line at a time,
  or shown in the pager when it is longer than the screen.

  `style` names how the pager draws the lines: `nil`, as they are, or `:help`, help's own text,
  its headings bold (`Redoubt.Shell.Help`). Only the pager reads it, and only those two values
  mean anything; anything made from the lines (`grep`, `head`) is plain.
  """

  defstruct [:source, style: nil]

  @type t :: %__MODULE__{source: Enumerable.t(), style: nil | :help}

  @doc false
  def new(source, style \\ nil), do: %__MODULE__{source: source, style: style}

  defimpl Enumerable do
    def reduce(%{source: source}, acc, fun), do: Enumerable.reduce(source, acc, fun)
    def count(_lines), do: {:error, __MODULE__}
    def member?(_lines, _line), do: {:error, __MODULE__}
    def slice(_lines), do: {:error, __MODULE__}
  end

  defimpl Inspect do
    # Inspecting must not read: the lines may be a large file, or a stream that runs once.
    def inspect(_lines, _opts), do: "#Lines<...>"
  end
end
