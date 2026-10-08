defmodule Redoubt.Screen.Widget.List do
  @moduledoc """
  A list with a selection, which is also the checklist and the radio list (`:select`):
  - `:none`, a list: Enter chooses the selected item;
  - `:one`, a radio list: Space marks the selected item, and Enter chooses the marked one (the
    selected one, if none is marked);
  - `:many`, a checklist: Space marks or unmarks the selected item, and Enter chooses the marked
    ones, in the list's order.

  The arrows, Page Up and Down, Home and End move. An item is shown as text: a string as it is,
  anything else inspected, with any control character drawn visibly. Enter ends with
  `{:done, value, list}`; a key the list does not take is `:pass`.
  """

  alias Redoubt.Screen.Widgets
  alias Redoubt.Screen.Widget.Theme
  alias Redoubt.Term.{Buffer, Text, Width}

  defstruct values: {}, labels: [], select: :none, selected: 0, marked: MapSet.new(), page: 10

  @type t :: %__MODULE__{}

  @doc """
  A list of `items`. Options: `:select` (`:none`, `:one` or `:many`), `:selected` (an index),
  `:marked` (indexes), `:page` (the rows Page Up and Down move; 10).
  """
  @spec new([term()], keyword()) :: t()
  def new(items, opts \\ []) do
    # As drawn: any control character in an item is drawn visibly, and measured so.
    labels = Enum.map(items, &(&1 |> label() |> Text.visible()))
    last = max(length(items) - 1, 0)

    %__MODULE__{
      values: List.to_tuple(items),
      labels: labels,
      select: Keyword.get(opts, :select, :none),
      selected: opts |> Keyword.get(:selected, 0) |> max(0) |> min(last),
      marked: MapSet.new(Keyword.get(opts, :marked, [])),
      page: Keyword.get(opts, :page, 10)
    }
  end

  defp label(item) when is_binary(item), do: item
  defp label(item), do: inspect(item)

  @doc "The number of items."
  @spec count(t()) :: non_neg_integer()
  def count(list), do: tuple_size(list.values)

  @doc "The columns the widest item takes as drawn, its mark included."
  @spec width(t()) :: non_neg_integer()
  def width(list),
    do: (list.labels |> Enum.map(&Width.columns/1) |> Enum.max(fn -> 0 end)) + mark_width(list.select)

  @doc "The value of the selected item, or `nil` when there is none."
  @spec selected(t()) :: term()
  def selected(%{values: {}}), do: nil
  def selected(list), do: elem(list.values, list.selected)

  @doc "The rows Page Up and Down move: a screen sets them to the rows the list is drawn in."
  @spec page(t(), pos_integer()) :: t()
  def page(list, rows), do: %{list | page: max(rows, 1)}

  @doc "A key for the list: `{:cont, list}`, `{:done, value, list}`, or `:pass`."
  @spec key(t(), term()) :: {:cont, t()} | {:done, term(), t()} | :pass
  def key(%{values: {}}, _key), do: :pass

  def key(list, {:key, key, []}) do
    case key do
      :up -> move(list, -1)
      :down -> move(list, 1)
      :page_up -> move(list, -list.page)
      :page_down -> move(list, list.page)
      :home -> move(list, -list.selected)
      :end -> move(list, count(list))
      " " -> toggle(list)
      :enter -> {:done, chosen(list), list}
      _other -> :pass
    end
  end

  def key(_list, _key), do: :pass

  defp move(list, delta) do
    {:cont, %{list | selected: (list.selected + delta) |> max(0) |> min(count(list) - 1)}}
  end

  defp toggle(%{select: :none}), do: :pass
  defp toggle(%{select: :one} = list), do: {:cont, %{list | marked: MapSet.new([list.selected])}}

  defp toggle(%{select: :many, marked: marked, selected: i} = list) do
    marked = if MapSet.member?(marked, i), do: MapSet.delete(marked, i), else: MapSet.put(marked, i)
    {:cont, %{list | marked: marked}}
  end

  defp chosen(%{select: :none} = list), do: selected(list)

  defp chosen(%{select: :one} = list) do
    case MapSet.to_list(list.marked) do
      [i] -> elem(list.values, i)
      _none -> selected(list)
    end
  end

  defp chosen(%{select: :many} = list),
    do: for(i <- list.marked |> MapSet.to_list() |> Enum.sort(), do: elem(list.values, i))

  @doc """
  Draws the list in `rect`, a row an item, scrolled to keep the selection in view; the selected
  row is drawn across its width in the theme's `:selected` when the list has the focus.
  """
  @spec draw(t(), Buffer.t(), Buffer.rect(), Theme.t(), boolean()) :: :ok
  def draw(list, buffer, {x, y, w, h}, theme, focused) do
    offset = if list.selected < h, do: 0, else: list.selected - h + 1
    normal = Theme.style(theme, :normal)
    selected = Theme.style(theme, :selected)

    list.labels
    |> Enum.drop(offset)
    |> Enum.take(h)
    |> Enum.with_index(offset)
    |> Enum.each(fn {item, i} ->
      style = if focused and i == list.selected, do: selected, else: normal
      Widgets.label(buffer, {x, y + i - offset, w, 1}, " " <> mark(list, i) <> item, style, pad: true)
    end)
  end

  defp mark(%{select: :none}, _i), do: ""
  defp mark(%{select: :one} = list, i), do: if(MapSet.member?(list.marked, i), do: "(•) ", else: "( ) ")
  defp mark(%{select: :many} = list, i), do: if(MapSet.member?(list.marked, i), do: "[x] ", else: "[ ] ")

  defp mark_width(:none), do: 0
  defp mark_width(_select), do: 4
end
