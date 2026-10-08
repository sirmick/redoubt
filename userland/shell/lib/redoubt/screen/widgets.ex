defmodule Redoubt.Screen.Widgets do
  @moduledoc """
  Widgets are functions, not processes (docs/userland/shell.md, "Full-screen programs"): each
  draws into a rectangle of a screen's buffer from what it is given. These are the ones that hold
  nothing: a box, a status line, a label and the completion pop-up. Those that take keys are
  modules of their own under `Redoubt.Screen.Widget`, each a struct and the functions `key` and
  `draw`.

  Every text a widget draws is made visible first (`Redoubt.Term.Text.visible/1`), so a control
  character in a title or an item is drawn as `^[` and the like, and fitted to its columns by
  `Redoubt.Term.Width`. A style comes from the code that draws, a theme's role
  (`Redoubt.Screen.Widget.Theme`), and never from the text.
  """

  alias Redoubt.Screen.Widget
  alias Redoubt.Screen.Widget.Theme
  alias Redoubt.Term.{Buffer, Text, Width}

  @doc """
  A box: a border in box drawing around `rect`, a title in its top border, and a shadow one
  cell to its right and below. Options: `:title`, `:style`, `:shadow` (`true` for the plain
  theme's, a style, or `false`).
  """
  @spec box(Buffer.t(), Buffer.rect(), keyword()) :: :ok
  def box(buffer, rect, opts \\ [])

  def box(buffer, {x, y, w, h}, opts) when w >= 2 and h >= 2 do
    style = Keyword.get(opts, :style, Buffer.plain())

    case Keyword.get(opts, :shadow, true) do
      false ->
        :ok

      shadow ->
        shadow = if shadow == true, do: Theme.style(Theme.plain(), :shadow), else: shadow
        Buffer.fill(buffer, {x + w, y + 1, 1, h}, " ", shadow)
        Buffer.fill(buffer, {x + 1, y + h, w, 1}, " ", shadow)
    end

    Buffer.fill(buffer, {x + 1, y + 1, w - 2, h - 2}, " ", style)
    Buffer.fill(buffer, {x + 1, y, w - 2, 1}, "─", style)
    Buffer.fill(buffer, {x + 1, y + h - 1, w - 2, 1}, "─", style)
    Buffer.fill(buffer, {x, y + 1, 1, h - 2}, "│", style)
    Buffer.fill(buffer, {x + w - 1, y + 1, 1, h - 2}, "│", style)
    Buffer.put(buffer, x, y, "┌", style)
    Buffer.put(buffer, x + w - 1, y, "┐", style)
    Buffer.put(buffer, x, y + h - 1, "└", style)
    Buffer.put(buffer, x + w - 1, y + h - 1, "┘", style)

    case Keyword.get(opts, :title) do
      nil -> :ok
      title -> label(buffer, {x + 2, y, max(w - 4, 0), 1}, " " <> title <> " ", style)
    end

    :ok
  end

  def box(_buffer, _rect, _opts), do: :ok

  @doc "A status line: `text` across `rect`'s first row, reversed (or in `style`)."
  @spec status(Buffer.t(), Buffer.rect(), String.t(), Buffer.style()) :: :ok
  def status(buffer, {x, y, w, _h}, text, style \\ Buffer.style(reversed: true)) do
    label(buffer, {x, y, w, 1}, " " <> text, style, pad: true)
    :ok
  end

  @doc """
  The completion pop-up: `list` (a `Redoubt.Screen.Widget.List`) in a box just below the cell
  `{x, y}`, or above it when there is no room below, inside `bounds`. At most `:rows` items
  (8) show at once; the list scrolls to keep its selection in view.
  """
  @spec popup(
          Buffer.t(),
          {non_neg_integer(), non_neg_integer()},
          Widget.List.t(),
          Buffer.rect(),
          Theme.t(),
          keyword()
        ) ::
          :ok
  def popup(buffer, {x, y}, list, {bx, by, bw, bh}, theme, opts \\ []) do
    rows = min(Widget.List.count(list), Keyword.get(opts, :rows, 8))
    {w, h} = {min(Widget.List.width(list) + 3, bw), rows + 2}
    below = by + bh - (y + 1)
    above = y - by
    top = if h <= below or below >= above, do: y + 1, else: max(y - h, by)
    h = min(h, if(top > y, do: below, else: above))
    left = x |> min(bx + bw - w) |> max(bx)

    if w >= 3 and h >= 3 do
      box(buffer, {left, top, w, h}, style: Theme.style(theme, :menu), shadow: false)
      Widget.List.draw(list, buffer, {left + 1, top + 1, w - 2, h - 2}, menu_theme(theme), true)
    end

    :ok
  end

  # A pop-up's list, in the menu's colours.
  defp menu_theme(theme), do: %{theme | normal: theme.menu, selected: theme.menu_selected}

  @doc """
  `text` made visible and drawn at `rect`'s top left, cut to its width; with `pad: true` the rest
  of the row is filled with spaces in the same style. The columns drawn, the padding included.
  """
  @spec label(Buffer.t(), Buffer.rect(), String.t(), Buffer.style(), keyword()) :: non_neg_integer()
  def label(buffer, {x, y, w, _h}, text, style, opts \\ []) do
    fitted = fit(Text.visible(text), w)
    written = if fitted == "", do: 0, else: Buffer.put(buffer, x, y, fitted, style)

    if Keyword.get(opts, :pad, false) and written < w do
      Buffer.fill(buffer, {x + written, y, w - written, 1}, " ", style)
      w
    else
      written
    end
  end

  @doc "The longest start of `text` that fits in `cols` columns."
  @spec fit(String.t(), non_neg_integer()) :: String.t()
  def fit(text, cols) do
    text
    |> String.graphemes()
    |> Enum.reduce_while({[], 0}, fn g, {acc, used} ->
      w = Width.grapheme(g)
      if used + w > cols, do: {:halt, {acc, used}}, else: {:cont, {[acc, g], used + w}}
    end)
    |> elem(0)
    |> IO.iodata_to_binary()
  end

  @doc """
  `text` made visible and broken into rows of at most `cols` columns: at spaces where it can be,
  inside a word longer than a row where it must. A newline starts a row.
  """
  @spec wrap(String.t(), pos_integer()) :: [String.t()]
  def wrap(text, cols) do
    text
    |> String.split("\n")
    |> Enum.flat_map(fn line -> line |> Text.visible() |> String.split(" ") |> rows(cols) end)
  end

  # Words laid into rows greedily, a word too long for a row cut where the row ends.
  defp rows(words, cols) do
    {rows, row} =
      Enum.reduce(words, {[], ""}, fn word, {rows, row} ->
        joined = if row == "", do: word, else: row <> " " <> word

        cond do
          Width.columns(joined) <= cols -> {rows, joined}
          row != "" and Width.columns(word) <= cols -> {[row | rows], word}
          true -> cut(joined, cols, rows)
        end
      end)

    Enum.reverse([row | rows])
  end

  defp cut(text, cols, rows) do
    head = fit(text, max(cols, 1))
    head = if head == "", do: String.first(text), else: head
    rest = binary_part(text, byte_size(head), byte_size(text) - byte_size(head))
    if Width.columns(rest) <= cols, do: {[head | rows], rest}, else: cut(rest, cols, [head | rows])
  end
end
