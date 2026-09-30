defmodule Redoubt.Util.Lines do
  @moduledoc """
  Lines of text, read as they are consumed.

  `cat/1` makes one over a file, and `grep/2` and `head/2` over another; nothing is read until
  something enumerates it, and a consumer that stops early stops the reading (a file is closed
  then). A line carries no line ending. At the prompt, a `%Lines{}` is printed a line at a time.
  """

  defstruct [:source]

  @type t :: %__MODULE__{source: Enumerable.t()}

  @doc false
  def new(source), do: %__MODULE__{source: source}

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
