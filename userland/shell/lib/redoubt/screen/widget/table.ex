defmodule Redoubt.Screen.Widget.Table do
  @moduledoc """
  A table: rows in columns, under a header drawn in the theme's `:header`. Cells are shown as
  `table` shows them (`Redoubt.Shell.Table.cells/1`), each column as wide as its widest cell and
  one space between columns, cut at the table's width.

  The arrows, Page Up and Down, Home and End move a selection, drawn across the row; Enter ends
  with `{:done, row, table}`, the row as it was given. A key the table does not take is `:pass`.
  """

  alias Redoubt.Screen.Widgets
  alias Redoubt.Screen.Widget.Theme
  alias Redoubt.Shell
  alias Redoubt.Term.Buffer

  defstruct rows: {}, cells: [], header: nil, widths: [], selected: 0, page: 10

  @type t :: %__MODULE__{}

  @doc """
  A table of `rows`. Options: `:header` (a row of column titles), `:page` (the rows Page Up and
  Down move; 10).
  """
  @spec new([term()], keyword()) :: t()
  def new(rows, opts \\ []) do
    cells = Enum.map(rows, &Shell.Table.cells/1)
    header = opts[:header] && Shell.Table.cells(opts[:header])

    %__MODULE__{
      rows: List.to_tuple(rows),
      cells: cells,
      header: header,
      widths: Shell.Table.widths(if(header, do: [header | cells], else: cells)),
      page: Keyword.get(opts, :page, 10)
    }
  end

  @doc "The value of the selected row, or `nil` when there is none."
  @spec selected(t()) :: term()
  def selected(%{rows: {}}), do: nil
  def selected(table), do: elem(table.rows, table.selected)

  @doc "The rows Page Up and Down move: a screen sets them to the rows the table is drawn in."
  @spec page(t(), pos_integer()) :: t()
  def page(table, rows), do: %{table | page: max(rows, 1)}

  @doc "A key for the table: `{:cont, table}`, `{:done, row, table}`, or `:pass`."
  @spec key(t(), term()) :: {:cont, t()} | {:done, term(), t()} | :pass
  def key(%{rows: {}}, _key), do: :pass

  def key(table, {:key, key, []}) do
    case key do
      :up -> move(table, -1)
      :down -> move(table, 1)
      :page_up -> move(table, -table.page)
      :page_down -> move(table, table.page)
      :home -> move(table, -table.selected)
      :end -> move(table, tuple_size(table.rows))
      :enter -> {:done, selected(table), table}
      _other -> :pass
    end
  end

  def key(_table, _key), do: :pass

  defp move(table, delta),
    do: {:cont, %{table | selected: (table.selected + delta) |> max(0) |> min(tuple_size(table.rows) - 1)}}

  @doc """
  Draws the table in `rect`: the header in its first row, if it has one, and the rows below,
  scrolled to keep the selection in view; the selected row in the theme's `:selected` when the
  table has the focus.
  """
  @spec draw(t(), Buffer.t(), Buffer.rect(), Theme.t(), boolean()) :: :ok
  def draw(table, buffer, {x, y, w, h}, theme, focused) do
    {y, h} =
      if table.header && h > 0 do
        header = Shell.Table.row(table.header, table.widths)
        Widgets.label(buffer, {x, y, w, 1}, header, Theme.style(theme, :header), pad: true)

        {y + 1, h - 1}
      else
        {y, h}
      end

    offset = if table.selected < h, do: 0, else: table.selected - h + 1

    table.cells
    |> Enum.drop(offset)
    |> Enum.take(max(h, 0))
    |> Enum.with_index(offset)
    |> Enum.each(fn {cells, i} ->
      style = Theme.style(theme, if(focused and i == table.selected, do: :selected, else: :normal))
      Widgets.label(buffer, {x, y + i - offset, w, 1}, Shell.Table.row(cells, table.widths), style, pad: true)
    end)
  end
end
