defmodule Redoubt.Screen.Pick do
  @moduledoc """
  A screen: `pick(items)`, `menuconfig`'s chooser (docs/userland/shell.md, "Full-screen
  programs"). A list in a box, centred; the arrows, Page Up and Down, Home and End move; Enter
  chooses and Esc leaves.
  """

  use Redoubt.Commandlet, area: "Screens"

  @behaviour Redoubt.Screen

  alias Redoubt.Screen.{Layout, Widgets}

  @hint "↑↓ move · Enter choose · Esc leave"

  @summary "Choose one item, on a screen"
  @help """
  Shows the items in a list on a screen of its own and returns the one chosen with Enter, or nil
  if Esc or the interrupt leaves it. The arrows, Page Up and Down, Home and End move. An item is
  shown as text: a string as it is, anything else inspected, with any control character in it
  drawn visibly. With title, the box has a title.
  """
  @args items: "the items to choose from", title: "a title, drawn in the box's border"
  @examples [
    {~S'pick(["red", "green", "blue"])', "one of three"},
    {~S'ls() |> pick(title: "open")', "a file of this directory"}
  ]
  defcommand pick(items :: lines, opts :: flags(title: string)) do
    case Enum.to_list(items) do
      [] -> nil
      items -> Redoubt.Screen.run(__MODULE__, {items, opts.title})
    end
  end

  @impl Redoubt.Screen
  def init({items, title}) do
    # As drawn: any control character in an item is drawn visibly, and measured so.
    labels = Enum.map(items, &(&1 |> label() |> Redoubt.Term.Text.visible()))
    %{items: List.to_tuple(items), labels: labels, title: title, selected: 0, page: 1}
  end

  defp label(item) when is_binary(item), do: item
  defp label(item), do: inspect(item)

  @impl Redoubt.Screen
  def update({:key, key, _mods}, state) do
    case key do
      :enter -> {:halt, elem(state.items, state.selected)}
      :esc -> {:halt, nil}
      :up -> move(state, -1)
      :down -> move(state, 1)
      :page_up -> move(state, -state.page)
      :page_down -> move(state, state.page)
      :home -> move(state, -state.selected)
      :end -> move(state, tuple_size(state.items))
      _other -> {:cont, state}
    end
  end

  # The list's rows on a screen of this size: the screen less the status line, a margin and the
  # box's borders. Page Up and Down move by them.
  def update({:resize, _cols, rows}, state), do: {:cont, %{state | page: max(rows - 5, 1)}}

  def update(_message, state), do: {:cont, state}

  defp move(state, delta) do
    last = tuple_size(state.items) - 1
    {:cont, %{state | selected: (state.selected + delta) |> max(0) |> min(last)}}
  end

  @impl Redoubt.Screen
  def view(state, buffer, {cols, rows}) do
    [body, status] = Layout.split({0, 0, cols, rows}, :rows, [:rest, {:fixed, 1}])
    widest = state.labels |> Enum.map(&Redoubt.Term.Width.columns/1) |> Enum.max()
    title = state.title || ""
    w = max(widest, Redoubt.Term.Width.columns(title) + 2) + 4
    h = length(state.labels) + 2
    # Room for the shadow, a cell right and below.
    {x, y, w, h} = Layout.centre(Layout.inset(body, 1), w, h)
    Widgets.box(buffer, {x, y, w, h}, title: state.title)
    {_, _, _, ih} = inner = Layout.inset({x, y, w, h}, 1)
    offset = if state.selected < ih, do: 0, else: state.selected - ih + 1
    Widgets.list(buffer, inner, state.labels, state.selected, offset)

    Widgets.status(buffer, status, @hint)
  end
end
