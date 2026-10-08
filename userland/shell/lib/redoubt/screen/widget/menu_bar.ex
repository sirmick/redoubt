defmodule Redoubt.Screen.Widget.MenuBar do
  @moduledoc """
  A menu bar with drop-downs, in the manner of QBasic. A menu is `{title, items}`, an item
  `{label, id}` or `:separator`.

  Closed, it takes only F10, which opens the first menu, and Alt with a menu's hot key, the first
  letter of its title (drawn in the theme's `:hotkey`), which opens that menu; every other key is
  `:pass`. Open, it takes every key: Left and Right move between menus, Up and Down along the
  items, Enter chooses one and ends with `{:done, id, bar}`, closing the menu, and Esc closes it.

  The bar is drawn along its rectangle's first row and an open menu below it, over whatever is
  there: a screen draws the bar last.
  """

  alias Redoubt.Screen.Widgets
  alias Redoubt.Screen.Widget.Theme
  alias Redoubt.Term.{Buffer, Text, Width}

  defstruct menus: [], open: nil, item: 0

  @type t :: %__MODULE__{}

  @doc "A bar of `menus`, closed."
  @spec new([{String.t(), [{String.t(), term()} | :separator]}]) :: t()
  def new(menus) do
    menus =
      for {title, items} <- menus do
        items =
          for item <- items,
              do: if(item == :separator, do: item, else: {Text.visible(elem(item, 0)), elem(item, 1)})

        {Text.visible(title), items}
      end

    %__MODULE__{menus: menus}
  end

  @doc "Whether a menu is open."
  @spec open?(t()) :: boolean()
  def open?(bar), do: bar.open != nil

  @doc "A key for the bar: `{:cont, bar}`, `{:done, id, bar}`, or `:pass`."
  @spec key(t(), term()) :: {:cont, t()} | {:done, term(), t()} | :pass
  def key(%{menus: []}, _key), do: :pass
  def key(%{open: nil} = bar, {:key, {:f, 10}, []}), do: {:cont, open(bar, 0)}

  def key(%{open: nil} = bar, {:key, letter, [:alt]}) when is_binary(letter) do
    case Enum.find_index(bar.menus, fn {title, _} -> hotkey(title) == String.downcase(letter) end) do
      nil -> :pass
      i -> {:cont, open(bar, i)}
    end
  end

  def key(%{open: nil}, _key), do: :pass

  def key(bar, {:key, key, []}) do
    count = length(bar.menus)

    case key do
      :esc -> {:cont, %{bar | open: nil}}
      :left -> {:cont, open(bar, Integer.mod(bar.open - 1, count))}
      :right -> {:cont, open(bar, Integer.mod(bar.open + 1, count))}
      :up -> {:cont, %{bar | item: step(items(bar), bar.item, -1)}}
      :down -> {:cont, %{bar | item: step(items(bar), bar.item, 1)}}
      :enter -> choose(bar)
      _other -> {:cont, bar}
    end
  end

  # Open, the bar is modal: a key it has no use for does nothing.
  def key(bar, _key), do: {:cont, bar}

  defp open(bar, i), do: %{bar | open: i, item: step(items(bar.menus, i), -1, 1)}

  defp items(bar), do: items(bar.menus, bar.open)
  defp items(menus, i), do: menus |> Enum.at(i) |> elem(1)

  # The next item from `at` in the direction of `delta` that is not a separator; `at` if none.
  defp step(items, at, delta) do
    Enum.find(
      Stream.iterate(at + delta, &(&1 + delta)) |> Enum.take_while(&(&1 >= 0 and &1 < length(items))),
      at,
      &(Enum.at(items, &1) != :separator)
    )
  end

  defp choose(bar) do
    case Enum.at(items(bar), bar.item) do
      {_label, id} -> {:done, id, %{bar | open: nil}}
      _none -> {:cont, bar}
    end
  end

  defp hotkey(title), do: title |> String.first() |> to_string() |> String.downcase()

  @doc "Draws the bar along `rect`'s first row, and the open menu below it, inside `rect`."
  @spec draw(t(), Buffer.t(), Buffer.rect(), Theme.t()) :: :ok
  def draw(bar, buffer, {x, y, w, h}, theme) do
    menu = Theme.style(theme, :menu)
    Buffer.fill(buffer, {x, y, w, 1}, " ", menu)

    Enum.reduce(Enum.with_index(bar.menus), x + 1, fn {{title, items}, i}, at ->
      open = bar.open == i
      style = if open, do: Theme.style(theme, :menu_selected), else: menu
      written = Widgets.label(buffer, {at, y, max(x + w - at, 0), 1}, " " <> title <> " ", style)

      if open,
        do: drop_down(bar, items, buffer, at, {x, y + 1, w, h - 1}, theme),
        else: hotkey(buffer, {at + 1, y, max(x + w - at - 1, 0), 1}, title, theme)

      at + written
    end)

    :ok
  end

  # The title's first letter, which opens its menu with Alt.
  defp hotkey(_buffer, _rect, "", _theme), do: :ok

  defp hotkey(buffer, rect, title, theme),
    do: Widgets.label(buffer, rect, String.first(title), Theme.style(theme, :hotkey))

  defp drop_down(bar, items, buffer, at, {bx, by, bw, bh}, theme) do
    widest =
      items
      |> Enum.map(fn
        {label, _} -> Width.columns(label)
        :separator -> 0
      end)
      |> Enum.max(fn -> 0 end)

    {w, h} = {min(widest + 4, bw), min(length(items) + 2, bh)}
    # A screen too small for the drop-down's border draws none of it.
    if w >= 3 and h >= 3,
      do: drop_down(bar, items, buffer, {at |> min(bx + bw - w) |> max(bx), by, w, h}, theme)
  end

  defp drop_down(bar, items, buffer, {left, by, w, h}, theme) do
    menu = Theme.style(theme, :menu)
    Widgets.box(buffer, {left, by, w, h}, style: menu, shadow: Theme.style(theme, :shadow))

    items
    |> Enum.take(max(h - 2, 0))
    |> Enum.with_index()
    |> Enum.each(fn
      {:separator, i} ->
        Buffer.fill(buffer, {left + 1, by + 1 + i, w - 2, 1}, "─", menu)
        Buffer.put(buffer, left, by + 1 + i, "├", menu)
        Buffer.put(buffer, left + w - 1, by + 1 + i, "┤", menu)

      {{label, _id}, i} ->
        style = if i == bar.item, do: Theme.style(theme, :menu_selected), else: menu
        Widgets.label(buffer, {left + 1, by + 1 + i, w - 2, 1}, " " <> label, style, pad: true)
    end)
  end
end
