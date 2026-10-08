defmodule Redoubt.Screen.Widget.Input do
  @moduledoc """
  A one-line text input with a cursor. What is typed is inserted at the cursor; Left and Right,
  Home and End (and Ctrl+A, Ctrl+E) move it; Backspace and Delete remove a character; Ctrl+U
  removes what is before the cursor and Ctrl+K what is after it. Enter ends with
  `{:done, text, input}`; a key the input does not take is `:pass`.

  The text is held as typed and drawn visibly, so a control character in it is drawn as `^[` and
  the like. The cursor is a cell drawn in the theme's `:cursor` style: the terminal's own cursor
  stays hidden while a screen is in front. A text wider than the input scrolls to keep the
  cursor in view.
  """

  alias Redoubt.Screen.Widgets
  alias Redoubt.Screen.Widget.Theme
  alias Redoubt.Term.{Buffer, Text, Width}

  # The graphemes before the cursor, nearest first, and those after it.
  defstruct before: [], after: []

  @type t :: %__MODULE__{}

  @doc "An input holding `text`, the cursor at its end."
  @spec new(String.t()) :: t()
  def new(text \\ ""), do: %__MODULE__{before: text |> String.graphemes() |> Enum.reverse()}

  @doc "The text."
  @spec value(t()) :: String.t()
  def value(input), do: IO.iodata_to_binary([Enum.reverse(input.before), input.after])

  @doc "A key for the input: `{:cont, input}`, `{:done, text, input}`, or `:pass`."
  @spec key(t(), term()) :: {:cont, t()} | {:done, String.t(), t()} | :pass
  def key(input, {:key, :enter, []}), do: {:done, value(input), input}
  def key(input, {:key, :left, []}), do: cont(left(input))
  def key(input, {:key, :right, []}), do: cont(right(input))
  def key(input, {:key, :home, []}), do: home(input)
  def key(input, {:key, "a", [:ctrl]}), do: home(input)
  def key(input, {:key, :end, []}), do: end_(input)
  def key(input, {:key, "e", [:ctrl]}), do: end_(input)
  def key(%{before: [_ | before]} = input, {:key, :backspace, []}), do: cont(%{input | before: before})
  def key(input, {:key, :backspace, []}), do: cont(input)
  def key(%{after: [_ | rest]} = input, {:key, :delete, []}), do: cont(%{input | after: rest})
  def key(input, {:key, :delete, []}), do: cont(input)
  def key(input, {:key, "u", [:ctrl]}), do: cont(%{input | before: []})
  def key(input, {:key, "k", [:ctrl]}), do: cont(%{input | after: []})

  def key(input, {:key, g, mods}) when is_binary(g) and mods in [[], [:shift]],
    do: cont(%{input | before: [g | input.before]})

  def key(_input, _key), do: :pass

  defp cont(input), do: {:cont, input}

  defp left(%{before: [g | before]} = input), do: %{input | before: before, after: [g | input.after]}
  defp left(input), do: input

  defp right(%{after: [g | rest]} = input), do: %{input | before: [g | input.before], after: rest}
  defp right(input), do: input

  defp home(input), do: cont(%{input | before: [], after: Enum.reverse(input.before, input.after)})
  defp end_(input), do: cont(%{input | before: Enum.reverse(input.after, input.before), after: []})

  @doc """
  Draws the input along `rect`'s first row in the theme's `:input` style, and, when it has the
  focus, its cursor.
  """
  @spec draw(t(), Buffer.t(), Buffer.rect(), Theme.t(), boolean()) :: :ok
  def draw(_input, _buffer, {_x, _y, w, _h}, _theme, _focused) when w < 1, do: :ok

  def draw(input, buffer, {x, y, w, _h}, theme, focused) do
    style = Theme.style(theme, :input)
    # Each grapheme as drawn: a control character takes the columns of its visible form.
    before = Enum.map(input.before, &Text.visible/1)
    # The graphemes before the cursor that are shown: as many as leave the cursor's cell in view.
    shown = shown(before, w - 1, [], 0)
    {at, rest} = {Width.columns(IO.iodata_to_binary(shown)), Enum.map(input.after, &Text.visible/1)}
    Widgets.label(buffer, {x, y, w, 1}, IO.iodata_to_binary([shown | rest]), style, pad: true)

    if focused do
      under = Widgets.fit(List.first(rest, " "), w - at)
      under = if under == "", do: " ", else: under
      Widgets.label(buffer, {x + at, y, w - at, 1}, under, Theme.style(theme, :cursor))
    end

    :ok
  end

  # The nearest graphemes of `before` (nearest first) that fit in `cols`, in order.
  defp shown([g | before], cols, acc, used) do
    w = Width.columns(g)
    if used + w > cols, do: acc, else: shown(before, cols, [g | acc], used + w)
  end

  defp shown([], _cols, acc, _used), do: acc
end
