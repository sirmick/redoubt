defmodule Redoubt.Editor.Syntax.Markdown do
  @moduledoc """
  Markdown, for the editor's highlighting (`Redoubt.Editor.Syntax`): `.md`. A heading's line is
  a heading; a fenced block, its fences included, and a `code span` are strings.
  """

  @behaviour Redoubt.Editor.Syntax

  alias Redoubt.Editor.Syntax

  @impl Syntax
  def line(text, {:fence, fence} = state) do
    if String.starts_with?(String.trim_leading(text), fence),
      do: {[{text, :string}], :code},
      else: {[{text, :string}], state}
  end

  def line(text, :code) do
    trimmed = String.trim_leading(text)

    cond do
      fence = Enum.find(["```", "~~~"], &String.starts_with?(trimmed, &1)) ->
        {[{text, :string}], {:fence, fence}}

      heading?(text) ->
        {[{text, :heading}], :code}

      true ->
        {spans(text, []), :code}
    end
  end

  # One to six `#`, then a space or the line's end.
  defp heading?(text) do
    hashes = text |> String.graphemes() |> Enum.take_while(&(&1 == "#")) |> length()
    rest = binary_part(text, hashes, byte_size(text) - hashes)
    hashes in 1..6 and (rest == "" or String.starts_with?(rest, " "))
  end

  # Text, and `code spans` in it; a ` with no closing one is text.
  defp spans("", acc), do: Enum.reverse(acc)

  defp spans(text, acc) do
    with [before, rest] <- :binary.split(text, "`"),
         [code, rest] <- :binary.split(rest, "`") do
      spans(rest, [{"`" <> code <> "`", :string}, {before, :normal} | acc])
    else
      _ -> Enum.reverse([{text, :normal} | acc])
    end
  end
end
