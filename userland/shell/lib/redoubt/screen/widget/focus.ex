defmodule Redoubt.Screen.Widget.Focus do
  @moduledoc """
  A focus ring over named widgets (docs/userland/shell.md, "Widgets, focus and themes"): Tab
  moves the focus to the next, Shift+Tab to the one before, round the ring; any other key goes to
  the widget with the focus, through its module's `key/2`.
  """

  defstruct ring: [], at: 0

  @type t :: %__MODULE__{}

  @doc "A ring over `names`, in order, the first focused."
  @spec new([term()]) :: t()
  def new(names), do: %__MODULE__{ring: names}

  @doc "The name of the widget with the focus."
  @spec focused(t()) :: term()
  def focused(%{ring: []}), do: nil
  def focused(focus), do: Enum.at(focus.ring, focus.at)

  @doc """
  A key for the ring and the widgets in `widgets`, a map from name to widget:
  `{:cont, focus, widgets}`, `{:done, name, value, focus, widgets}` when the focused widget ended
  with `value`, or `:pass` when neither took the key.
  """
  @spec key(t(), map(), term()) ::
          {:cont, t(), map()} | {:done, term(), term(), t(), map()} | :pass
  def key(%{ring: []}, _widgets, _key), do: :pass
  def key(focus, widgets, {:key, :tab, []}), do: {:cont, step(focus, 1), widgets}
  def key(focus, widgets, {:key, :tab, [:shift]}), do: {:cont, step(focus, -1), widgets}

  def key(focus, widgets, key) do
    name = focused(focus)
    widget = Map.fetch!(widgets, name)

    case widget.__struct__.key(widget, key) do
      {:cont, widget} -> {:cont, focus, Map.put(widgets, name, widget)}
      {:done, value, widget} -> {:done, name, value, focus, Map.put(widgets, name, widget)}
      :pass -> :pass
    end
  end

  defp step(focus, delta), do: %{focus | at: Integer.mod(focus.at + delta, length(focus.ring))}
end
