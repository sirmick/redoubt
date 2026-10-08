defmodule Redoubt.Editor.View do
  @moduledoc """
  How the editor shows a line (docs/userland/shell.md, "The editor"): as runs of text, each in
  one role of the theme, to draw from a column of the window. A tab is the spaces to the next
  stop; any other control or bidirectional character is drawn visibly, as `^[` or `<U+202E>`
  (`Redoubt.Term.Text`), and is as wide as that. What the file holds sets no style: the role
  of each run is the selection's, the cursor's or the text's.
  """

  alias Redoubt.Term.{Text, Width}

  @tab 4

  @typedoc "A run: its column in the window, its text, and its role."
  @type run :: {non_neg_integer(), String.t(), :normal | :selected | :cursor}

  @doc "The columns a tab stop is apart."
  @spec tab() :: pos_integer()
  def tab, do: @tab

  @doc "The column on screen, before any scroll, where grapheme `col` of `line` starts."
  @spec column(String.t(), non_neg_integer()) :: non_neg_integer()
  def column(line, col) do
    line |> String.graphemes() |> Enum.take(col) |> Enum.reduce(0, &(&2 + Width.columns(piece(&1, &2))))
  end

  @doc """
  The runs that draw `line` in a window `width` columns wide whose first column is the line's
  column `left`. `selected` is the range of graphemes selected on this line, `first..last` or
  `nil`; `cursor` is the grapheme the cursor is on, or `nil`. The cursor at the line's end is a
  blank cell; a selection that runs on to the next line shows a blank cell at this one's end.
  """
  @spec runs(
          String.t(),
          non_neg_integer(),
          pos_integer(),
          Range.t() | nil,
          non_neg_integer() | nil,
          boolean()
        ) ::
          [run()]
  def runs(line, left, width, selected, cursor, selected_end? \\ false) do
    graphemes = String.graphemes(line)
    count = length(graphemes)

    {pieces, at} =
      Enum.reduce(Enum.with_index(graphemes), {[], 0}, fn {g, i}, {acc, at} ->
        text = piece(g, at)
        {[{at, text, role(i, selected, cursor)} | acc], at + Width.columns(text)}
      end)

    tail =
      cond do
        cursor == count -> [{at, " ", :cursor}]
        selected_end? -> [{at, " ", :selected}]
        true -> []
      end

    (Enum.reverse(pieces) ++ tail)
    |> Enum.flat_map(&clip(&1, left, width))
    |> merge()
  end

  # What a grapheme is drawn as, at column `at`.
  defp piece("\t", at), do: String.duplicate(" ", @tab - rem(at, @tab))
  defp piece(g, _at), do: Text.visible(g)

  defp role(i, _selected, i), do: :cursor
  defp role(i, %Range{} = selected, _cursor), do: if(i in selected, do: :selected, else: :normal)
  defp role(_i, nil, _cursor), do: :normal

  # A piece as it falls in the window: whole, or as blanks where only part of it shows.
  defp clip({at, text, role}, left, width) do
    w = Width.columns(text)
    {from, to} = {max(at, left), min(at + w, left + width)}

    cond do
      to <= from -> []
      from == at and to == at + w -> [{at - left, text, role}]
      true -> [{from - left, String.duplicate(" ", to - from), role}]
    end
  end

  # Neighbouring runs of one role, one run.
  defp merge(runs) do
    runs
    |> Enum.reduce([], fn
      {_at, text, role}, [{start, prev, role} | rest] -> [{start, prev <> text, role} | rest]
      run, acc -> [run | acc]
    end)
    |> Enum.reverse()
  end
end
