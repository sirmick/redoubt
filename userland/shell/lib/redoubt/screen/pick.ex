defmodule Redoubt.Screen.Pick do
  @moduledoc """
  A screen: `pick(items)`, `menuconfig`'s chooser (docs/userland/shell.md, "Full-screen
  programs"). A list in a box, centred; the arrows, Page Up and Down, Home and End move; Enter
  chooses and Esc leaves.
  """

  use Redoubt.Commandlet, area: "Screens"

  @behaviour Redoubt.Screen

  alias Redoubt.Screen.{Layout, Widget, Widgets}
  alias Redoubt.Screen.Widget.Theme

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
  def init({items, title}), do: %{list: Widget.List.new(items), title: title}

  @impl Redoubt.Screen
  # A key counts with any modifiers held: Alt+Esc leaves, Shift+Down moves.
  def update({:key, :esc, _mods}, _state), do: {:halt, nil}

  def update({:key, key, _mods}, state) do
    case Widget.List.key(state.list, {:key, key, []}) do
      {:done, value, _list} -> {:halt, value}
      {:cont, list} -> {:cont, %{state | list: list}}
      :pass -> {:cont, state}
    end
  end

  # The list's rows on a screen of this size: the screen less the status line, a margin and the
  # box's borders. Page Up and Down move by them.
  def update({:resize, _cols, rows}, state),
    do: {:cont, %{state | list: Widget.List.page(state.list, rows - 5)}}

  def update(_message, state), do: {:cont, state}

  @impl Redoubt.Screen
  def view(state, buffer, {cols, rows}) do
    [body, status] = Layout.split({0, 0, cols, rows}, :rows, [:rest, {:fixed, 1}])
    title = state.title || ""
    w = max(Widget.List.width(state.list), Redoubt.Term.Width.columns(title) + 2) + 4
    h = Widget.List.count(state.list) + 2
    # Room for the shadow, a cell right and below.
    rect = Layout.centre(Layout.inset(body, 1), w, h)
    Widgets.box(buffer, rect, title: state.title)
    Widget.List.draw(state.list, buffer, Layout.inset(rect, 1), Theme.plain(), true)
    Widgets.status(buffer, status, @hint)
  end
end
