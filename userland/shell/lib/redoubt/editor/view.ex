defmodule Redoubt.Editor.View do
  @moduledoc """
  How the editor shows a line (docs/userland/shell.md, "The editor"): as runs of text, each in
  one role of the theme, to draw from a column of the window. A tab is the spaces to the next
  stop; any other control or bidirectional character is drawn visibly, as `^[` or `<U+202E>`
  (`Redoubt.Term.Text`), and is as wide as that. What the file holds sets no style: the role
  of each run is the cursor's, the selection's, or the one its language's highlighting gave that
  part of the line (`Redoubt.Editor.Syntax`), one of the theme's.
  """

  alias Redoubt.Term.{Text, Width}

  @tab 4

  @typedoc "A run: its column in the window, its text, and its role."
  @type run :: {non_neg_integer(), String.t(), :selected | :cursor | Redoubt.Editor.Syntax.role()}

  @doc "The columns a tab stop is apart."
  @spec tab() :: pos_integer()
  def tab, do: @tab

  @doc """
  The column on screen, before any scroll, where grapheme `col` of `line` starts: only the
  graphemes before it are read.
  """
  @spec column(String.t(), non_neg_integer()) :: non_neg_integer()
  def column(line, col), do: column(line, col, 0)

  defp column(_line, 0, at), do: at

  defp column(line, col, at) do
    case String.next_grapheme(line) do
      {g, rest} -> column(rest, col - 1, at + Width.columns(piece(g, at)))
      nil -> at
    end
  end

  @doc """
  The runs that draw `line` in a window `width` columns wide whose first column is the line's
  column `left`. `selected` is the range of graphemes selected on this line, `first..last` or
  `nil`; `cursor` is the grapheme the cursor is on, or `nil`. The cursor at the line's end is a
  blank cell; a selection that runs on to the next line shows a blank cell at this one's end.
  `pieces` is the line cut by its highlighting, each piece with its role, or `nil` for plain text.
  Only the graphemes up to the window's right edge are read, so a draw costs what is left of the
  window and in it, not the line's length.
  """
  @spec runs(
          String.t(),
          non_neg_integer(),
          pos_integer(),
          Range.t() | nil,
          non_neg_integer() | nil,
          boolean(),
          [Redoubt.Editor.Syntax.piece()] | nil
        ) ::
          [run()]
  def runs(line, left, width, selected, cursor, selected_end? \\ false, pieces \\ nil) do
    walked = %{stop: left + width, selected: selected, cursor: cursor}
    {drawn, at, count, rest} = walk(line, walked, 0, 0, 0, ends(pieces), [])

    # The line's end, when the walk reached it before the window's edge.
    tail =
      cond do
        rest != "" -> []
        cursor == count -> [{at, " ", :cursor}]
        selected_end? -> [{at, " ", :selected}]
        true -> []
      end

    (Enum.reverse(drawn) ++ tail)
    |> Enum.flat_map(&clip(&1, left, width))
    |> merge()
  end

  # What a grapheme is drawn as, at column `at`.
  defp piece("\t", at), do: String.duplicate(" ", @tab - rem(at, @tab))
  defp piece(g, _at), do: Text.visible(g)

  # The graphemes from the line's start to the window's right edge, each drawn at its column in
  # its role: `i` counts graphemes, `byte` is where the grapheme starts in the line, and `ends` is
  # what is left of the highlighting's pieces, each grapheme taking the role of the piece it
  # starts in. Gives the pieces drawn, the column and grapheme reached, and the line not read.
  defp walk(text, %{stop: stop}, at, i, _byte, _ends, acc) when at >= stop, do: {acc, at, i, text}

  defp walk(text, walked, at, i, byte, ends, acc) do
    case String.next_grapheme(text) do
      nil ->
        {acc, at, i, ""}

      {g, rest} ->
        ends = Enum.drop_while(ends, fn {stop, _role} -> stop <= byte end)
        base = with [{_stop, role} | _] <- ends, do: role, else: (_ -> :normal)
        drawn = piece(g, at)
        acc = [{at, drawn, role(i, walked.selected, walked.cursor, base)} | acc]
        walk(rest, walked, at + Width.columns(drawn), i + 1, byte + byte_size(g), ends, acc)
    end
  end

  defp role(i, _selected, i, _base), do: :cursor
  defp role(i, %Range{} = selected, _cursor, base), do: if(i in selected, do: :selected, else: base)
  defp role(_i, nil, _cursor, base), do: base

  # Where each piece of the highlighting ends in the line, in bytes, with its role.
  defp ends(nil), do: []

  defp ends(pieces) do
    {ends, _at} =
      Enum.map_reduce(pieces, 0, fn {text, role}, at ->
        {{at + byte_size(text), role}, at + byte_size(text)}
      end)

    ends
  end

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
