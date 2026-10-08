defmodule Redoubt.Screen.Widget.Buttons do
  @moduledoc """
  A row of buttons, one of them the current one. Left and Right move along the row; Enter or
  Space presses the current button and ends with `{:done, id, buttons}`; a key the row does not
  take is `:pass`. A button is `{label, id}`, drawn as `< label >`.
  """

  alias Redoubt.Screen.Widgets
  alias Redoubt.Screen.Widget.Theme
  alias Redoubt.Term.{Buffer, Text, Width}

  defstruct buttons: [], current: 0

  @type t :: %__MODULE__{}

  @doc "A row of `buttons`, each `{label, id}`; the one at `current` (0) is current."
  @spec new([{String.t(), term()}], non_neg_integer()) :: t()
  def new(buttons, current \\ 0) do
    buttons = for {label, id} <- buttons, do: {"< " <> Text.visible(label) <> " >", id}
    %__MODULE__{buttons: buttons, current: current |> max(0) |> min(max(length(buttons) - 1, 0))}
  end

  @doc "The columns the row takes: its buttons, two apart."
  @spec width(t()) :: non_neg_integer()
  def width(%{buttons: buttons}),
    do: Enum.sum(for {label, _id} <- buttons, do: Width.columns(label)) + 2 * max(length(buttons) - 1, 0)

  @doc "A key for the row: `{:cont, buttons}`, `{:done, id, buttons}`, or `:pass`."
  @spec key(t(), term()) :: {:cont, t()} | {:done, term(), t()} | :pass
  def key(%{buttons: []}, _key), do: :pass
  def key(row, {:key, :left, []}), do: {:cont, %{row | current: max(row.current - 1, 0)}}

  def key(row, {:key, :right, []}),
    do: {:cont, %{row | current: min(row.current + 1, length(row.buttons) - 1)}}

  def key(row, {:key, k, []}) when k in [:enter, " "],
    do: {:done, row.buttons |> Enum.at(row.current) |> elem(1), row}

  def key(_row, _key), do: :pass

  @doc """
  Draws the row centred in `rect`'s first row; the current button is drawn in the theme's
  `:button_focused` when the row has the focus.
  """
  @spec draw(t(), Buffer.t(), Buffer.rect(), Theme.t(), boolean()) :: :ok
  def draw(row, buffer, {x, y, w, _h}, theme, focused) do
    left = x + max(div(w - width(row), 2), 0)

    row.buttons
    |> Enum.with_index()
    |> Enum.reduce(left, fn {{label, _id}, i}, at ->
      role = if focused and i == row.current, do: :button_focused, else: :button
      written = Widgets.label(buffer, {at, y, max(x + w - at, 0), 1}, label, Theme.style(theme, role))
      at + written + 2
    end)

    :ok
  end
end
