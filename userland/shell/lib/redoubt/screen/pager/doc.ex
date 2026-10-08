defmodule Redoubt.Screen.Pager.Doc do
  @moduledoc """
  What the pager shows, apart from drawing it: the lines read so far, whether their end has been
  read, and how they wrap into rows at a width. Pure, so its rules are tested on any VM.

  A line is shown as `Redoubt.Term.Text.visible/1` makes it. One wider than the screen goes on
  in further rows, each indented by the line's hanging indent (help's list items keep their
  text's column). A place in the document is `{line, row}`: a line, and one of its rows, both
  from 0.

  A style says how a line is shown: `nil`, as it is, or `:help`, help's own text
  (`Redoubt.Shell.Help.styled/1`), whose headings are bold. No other value means anything, so
  data cannot choose how it is drawn.
  """

  alias Redoubt.Term.{Text, Width}

  defstruct lines: :array.new(), ended: false, style: nil

  @type t :: %__MODULE__{}
  @type place :: {non_neg_integer(), non_neg_integer()}

  @doc "A document of no lines yet, drawn in `style`."
  @spec new(nil | :help) :: t()
  def new(style \\ nil), do: %__MODULE__{style: style}

  @doc """
  `lines` read after the others; with `ended`, the last there are. Each is kept as it is shown,
  styled once here.
  """
  @spec push(t(), [String.t()], boolean()) :: t()
  def push(doc, lines, ended \\ false) do
    array =
      Enum.reduce(lines, doc.lines, fn line, array ->
        :array.set(:array.size(array), style(doc.style, line), array)
      end)

    %{doc | lines: array, ended: doc.ended or ended}
  end

  defp style(:help, line) do
    {text, bold, indent} = Redoubt.Shell.Help.styled(line)
    {Text.visible(text), bold, indent}
  end

  defp style(_plain, line), do: {Text.visible(line), false, 0}

  @doc "How many lines have been read."
  @spec count(t()) :: non_neg_integer()
  def count(doc), do: :array.size(doc.lines)

  @doc "Line `i` as shown: its visible text, whether it is bold, and its hanging indent."
  @spec shown(t(), non_neg_integer()) :: {String.t(), boolean(), non_neg_integer()}
  def shown(doc, i), do: :array.get(i, doc.lines)

  @doc """
  `text` in rows of at most `cols` columns, the rows after the first starting with `indent`
  spaces (none when that would leave less than half the width). A wide grapheme the edge would
  cut starts the next row; an empty text is one empty row.
  """
  @spec wrap(String.t(), pos_integer(), non_neg_integer()) :: [String.t()]
  def wrap(text, cols, indent) do
    indent = if indent * 2 <= cols, do: indent, else: 0
    pad = String.duplicate(" ", indent)
    wrap(String.graphemes(text), cols, pad, indent, {[], 0, true}, [])
  end

  defp wrap([], _cols, _pad, _indent, {row, _used, _fresh}, rows),
    do: Enum.reverse([IO.iodata_to_binary(Enum.reverse(row)) | rows])

  defp wrap([g | rest] = gs, cols, pad, indent, {row, used, fresh}, rows) do
    w = Width.grapheme(g)

    if used + w > cols and not fresh do
      done = IO.iodata_to_binary(Enum.reverse(row))
      wrap(gs, cols, pad, indent, {[pad], indent, true}, [done | rows])
    else
      wrap(rest, cols, pad, indent, {[g | row], used + w, false}, rows)
    end
  end

  @doc "Line `i`'s rows at `cols`."
  @spec rows(t(), non_neg_integer(), pos_integer()) :: [String.t()]
  def rows(doc, i, cols) do
    {text, _bold, indent} = shown(doc, i)
    wrap(text, cols, indent)
  end

  @doc """
  Up to `h` rows from `place`, each `{line, text, bold}`: as many as the lines read hold.
  """
  @spec view(t(), place(), pos_integer(), non_neg_integer()) :: [{non_neg_integer(), String.t(), boolean()}]
  def view(doc, {line, row}, cols, h), do: view(doc, line, row, cols, h, [])

  defp view(_doc, _line, _row, _cols, 0, acc), do: Enum.reverse(acc)

  defp view(doc, line, row, cols, h, acc) do
    if line >= count(doc) do
      Enum.reverse(acc)
    else
      {_text, bold, _indent} = shown(doc, line)
      shown = doc |> rows(line, cols) |> Enum.drop(row) |> Enum.take(h)
      acc = Enum.reduce(shown, acc, &[{line, &1, bold} | &2])
      view(doc, line + 1, 0, cols, h - length(shown), acc)
    end
  end

  @doc "`n` rows on from `place`, no further than the last row read."
  @spec down(t(), place(), non_neg_integer(), pos_integer()) :: place()
  def down(doc, {line, row}, n, cols) do
    # The rows of this line below `row`; a whole line is stepped at once.
    below = length(rows(doc, line, cols)) - 1 - row

    cond do
      n <= below -> {line, row + n}
      line + 1 < count(doc) -> down(doc, {line + 1, 0}, n - below - 1, cols)
      true -> {line, row + below}
    end
  end

  @doc "`n` rows back from `place`, no further than the first."
  @spec up(t(), place(), non_neg_integer(), pos_integer()) :: place()
  def up(doc, {line, row}, n, cols) do
    cond do
      n <= row -> {line, row - n}
      line > 0 -> up(doc, {line - 1, length(rows(doc, line - 1, cols)) - 1}, n - row - 1, cols)
      true -> {0, 0}
    end
  end

  @doc """
  The place that shows the last row read at the bottom of `h` rows, or the first place when
  everything read fits.
  """
  @spec last(t(), pos_integer(), pos_integer()) :: place()
  def last(doc, cols, h) do
    case count(doc) do
      0 ->
        {0, 0}

      n ->
        bottom = {n - 1, length(rows(doc, n - 1, cols)) - 1}
        up(doc, bottom, h - 1, cols)
    end
  end

  @doc """
  The first line after `from` (`:next`) or before it (`:prev`) whose shown text holds `query`,
  among the lines read, or `nil`.
  """
  @spec find(t(), String.t(), integer(), :next | :prev) :: non_neg_integer() | nil
  def find(doc, query, from, :next), do: Enum.find((from + 1)..(count(doc) - 1)//1, &holds?(doc, &1, query))
  def find(doc, query, from, :prev), do: Enum.find((from - 1)..0//-1, &holds?(doc, &1, query))

  defp holds?(doc, i, query), do: doc |> shown(i) |> elem(0) |> String.contains?(query)
end
