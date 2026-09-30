defmodule Redoubt.Shell.Table do
  @moduledoc """
  `table`, imported at the prompt: rows laid out as text, in columns, fitted to the terminal's
  width. It needs no screen, so it draws nothing itself: it gives lines.
  """

  use Redoubt.Commandlet, area: "Text"

  alias Redoubt.Term.{Text, Width}
  alias Redoubt.Util.Lines

  @summary "Lay rows out as a table"
  @help """
  Lays rows out as a table, each column as wide as its widest cell and one space between columns.
  A row is a list or a tuple of cells, or one value; a cell is any value, shown as text. With
  header, the first row is the table's header; with title, the table has a border with the title
  in it. What it gives is lines, cut at the terminal's width.
  """
  @args rows: "the rows", header: "take the first row as the header", title: "a title, drawn in a border"
  @examples [
    {~S'table([["name", "size"], ["a.log", 12]], header: true)', "two columns, under a header"},
    {~S'ls_r() |> Enum.map(&[&1, stat(&1).size]) |> table(title: "sizes")',
     "every path below here, and its size"}
  ]
  defcommand table(rows :: lines, opts :: flags(header: boolean, title: string)) do
    rows = Enum.map(rows, &cells/1)
    widths = columns(rows)
    # A header is laid out as the first row: in text, nothing sets it apart until screens do.
    body = Enum.map(rows, &row(&1, widths))
    terminal = terminal_width()

    lines =
      case opts.title && Text.visible(opts.title) do
        nil -> Enum.map(body, &(&1 |> clip(terminal) |> String.trim_trailing()))
        title -> boxed(body, title, widths, terminal)
      end

    Lines.new(lines)
  end

  # A border around the rows, the title in its top edge, as wide as the rows or the title,
  # whichever is wider, and no wider than the terminal.
  defp boxed(body, title, widths, terminal) do
    rows = Enum.sum(widths) + max(length(widths) - 1, 0)
    inner = rows |> max(Width.columns(title)) |> min(max(terminal - 2, 1))
    top = "┌" <> pad(clip(title, inner), inner, "─") <> "┐"
    bottom = "└" <> String.duplicate("─", inner) <> "┘"
    [top | Enum.map(body, &("│" <> pad(clip(&1, inner), inner, " ") <> "│"))] ++ [bottom]
  end

  # A row's cells, each padded to its column's width, one space between them.
  defp row(cells, widths) do
    widths
    |> Enum.with_index()
    |> Enum.map_join(" ", fn {width, c} -> pad(Enum.at(cells, c, ""), width, " ") end)
  end

  # `text` made up to `width` columns with `fill`.
  defp pad(text, width, fill), do: text <> String.duplicate(fill, max(width - Width.columns(text), 0))

  # `text` cut at `width` columns; a wide grapheme the cut would split becomes a space.
  defp clip(text, width) do
    text
    |> String.graphemes()
    |> Enum.reduce_while({[], 0}, fn g, {acc, used} ->
      w = Width.columns(g)

      cond do
        used + w <= width -> {:cont, {[g | acc], used + w}}
        used < width -> {:halt, {[" " | acc], width}}
        true -> {:halt, {acc, used}}
      end
    end)
    |> elem(0)
    |> Enum.reverse()
    |> IO.iodata_to_binary()
  end

  # A row's cells, as text as it will be shown.
  defp cells(row) when is_list(row), do: Enum.map(row, &cell/1)
  defp cells(row) when is_tuple(row), do: row |> Tuple.to_list() |> cells()
  defp cells(value), do: [cell(value)]

  defp cell(text) when is_binary(text), do: Text.visible(text)
  defp cell(value) when is_atom(value) or is_number(value), do: value |> to_string() |> Text.visible()
  defp cell(value), do: value |> inspect() |> Text.visible()

  # Each column's width: its widest cell's.
  defp columns(rows) do
    count = rows |> Enum.map(&length/1) |> Enum.max(fn -> 0 end)

    for c <- 0..(count - 1)//1,
        do: rows |> Enum.map(&Width.columns(Enum.at(&1, c, ""))) |> Enum.max(fn -> 0 end)
  end

  defp terminal_width do
    case :io.columns() do
      {:ok, columns} -> columns
      {:error, _reason} -> 80
    end
  end
end
