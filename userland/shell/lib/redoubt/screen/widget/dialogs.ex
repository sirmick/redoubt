defmodule Redoubt.Screen.Widget.Dialogs do
  @moduledoc """
  A stack of modal dialogs (docs/userland/shell.md, "Widgets, focus and themes"). Keys go to the
  dialog on top; with none open, `key/2` passes every key to the screen. Each dialog is laid out
  in `draw/4` from the rectangle it is given, bottom to top, so a screen of a new size lays the
  stack out again at that size.

  The dialogs:
  - `message(id, title, text)`: the text and OK; ends with `:ok`;
  - `confirm(id, title, text)`: Yes and No; ends with `true` or `false`;
  - `prompt(id, title, text, value)`: a text input under the text, OK and Cancel; ends with the
    text typed, Enter in the input being OK, or `nil` for Cancel.

  Inside a dialog Tab and Shift+Tab move the focus between the input and the buttons
  (`Redoubt.Screen.Widget.Focus`). Esc closes the top dialog with `nil`. A dialog that ends is taken off
  the stack, and `key/2` says so: `{:closed, id, value, stack}`.
  """

  alias Redoubt.Screen.{Layout, Widget, Widgets}
  alias Redoubt.Screen.Widget.{Focus, Theme}
  alias Redoubt.Term.{Buffer, Text, Width}

  defstruct stack: []

  @type t :: %__MODULE__{}

  @doc "An empty stack."
  @spec new() :: t()
  def new, do: %__MODULE__{}

  @doc "Whether a dialog is open."
  @spec open?(t()) :: boolean()
  def open?(%{stack: stack}), do: stack != []

  @doc "`dialog` on top of the stack."
  @spec push(t(), map()) :: t()
  def push(dialogs, dialog), do: %{dialogs | stack: [dialog | dialogs.stack]}

  @doc "A message with OK."
  @spec message(term(), String.t(), String.t()) :: map()
  def message(id, title, text), do: dialog(id, title, text, %{buttons: Widget.Buttons.new([{"OK", :ok}])})

  @doc "A question with Yes and No."
  @spec confirm(term(), String.t(), String.t()) :: map()
  def confirm(id, title, text),
    do: dialog(id, title, text, %{buttons: Widget.Buttons.new([{"Yes", true}, {"No", false}])})

  @doc "A question answered with a line of text, starting as `value`."
  @spec prompt(term(), String.t(), String.t(), String.t()) :: map()
  def prompt(id, title, text, value \\ "") do
    buttons = Widget.Buttons.new([{"OK", :ok}, {"Cancel", nil}])
    dialog(id, title, text, %{input: Widget.Input.new(value), buttons: buttons})
  end

  defp dialog(id, title, text, widgets) do
    ring = Enum.filter([:input, :buttons], &Map.has_key?(widgets, &1))
    %{id: id, title: title, text: text, widgets: widgets, focus: Focus.new(ring)}
  end

  @doc """
  A key for the top dialog: `{:cont, dialogs}`, `{:closed, id, value, dialogs}` when it ended,
  or `:pass` when no dialog is open.
  """
  @spec key(t(), term()) :: {:cont, t()} | {:closed, term(), term(), t()} | :pass
  def key(%{stack: []}, _key), do: :pass
  def key(%{stack: [top | rest]}, {:key, :esc, []}), do: {:closed, top.id, nil, %__MODULE__{stack: rest}}

  def key(%{stack: [top | rest]} = dialogs, key) do
    case Focus.key(top.focus, top.widgets, key) do
      {:cont, focus, widgets} ->
        {:cont, %{dialogs | stack: [%{top | focus: focus, widgets: widgets} | rest]}}

      {:done, name, value, _focus, widgets} ->
        {:closed, top.id, result(name, value, widgets), %{dialogs | stack: rest}}

      # A modal dialog keeps the keys it does not take from the screen under it.
      :pass ->
        {:cont, dialogs}
    end
  end

  # Enter in a prompt's input is OK; its OK gives the text.
  defp result(:input, text, _widgets), do: text
  defp result(:buttons, :ok, %{input: input}), do: Widget.Input.value(input)
  defp result(:buttons, value, _widgets), do: value

  @doc "Draws the stack in `rect`, bottom to top, each dialog centred."
  @spec draw(t(), Buffer.t(), Buffer.rect(), Theme.t()) :: :ok
  def draw(dialogs, buffer, rect, theme) do
    dialogs.stack |> Enum.reverse() |> Enum.each(&draw_one(&1, buffer, rect, theme))
  end

  # The widest a dialog's text is laid out, in columns.
  @widest 60

  defp draw_one(dialog, buffer, {_, _, rw, rh} = rect, theme) do
    input? = Map.has_key?(dialog.widgets, :input)
    text = dialog.text |> Widgets.wrap(@widest) |> Enum.map(&Width.columns/1) |> Enum.max(fn -> 0 end)
    title = Width.columns(Text.visible(dialog.title)) + 2
    # The dialog's inside: room for the text, the title, the buttons and an input, within the screen.
    inside =
      [text, title, Widget.Buttons.width(dialog.widgets.buttons), if(input?, do: 30, else: 0)]
      |> Enum.max()
      |> min(max(rw - 6, 1))

    lines = Widgets.wrap(dialog.text, inside)
    extra = if input?, do: 2, else: 0
    # The border, a blank row and the buttons take four rows; what the screen leaves of the text shows.
    {x, y, w, h} = Layout.centre(rect, inside + 4, min(length(lines) + extra + 4, max(rh - 1, 0)))
    lines = Enum.take(lines, max(h - extra - 4, 0))
    if h >= 4, do: draw_in(dialog, lines, buffer, {x, y, w, h}, theme)
  end

  # A dialog drawn in `rect`, tall enough for its border and its buttons.
  defp draw_in(dialog, lines, buffer, {x, y, w, h}, theme) do
    style = Theme.style(theme, :dialog)
    buttons = dialog.widgets.buttons
    input? = Map.has_key?(dialog.widgets, :input)
    Widgets.box(buffer, {x, y, w, h}, title: dialog.title, style: style, shadow: Theme.style(theme, :shadow))

    {tx, ty, tw, _} = Layout.inset({x, y, w, h}, 1)

    lines
    |> Enum.with_index()
    |> Enum.each(fn {line, i} -> Widgets.label(buffer, {tx + 1, ty + i, tw - 2, 1}, line, style) end)

    focused = Focus.focused(dialog.focus)

    if input? do
      Widget.Input.draw(
        dialog.widgets.input,
        buffer,
        {tx + 1, ty + length(lines) + 1, tw - 2, 1},
        theme,
        focused == :input
      )
    end

    Widget.Buttons.draw(buttons, buffer, {tx, y + h - 2, tw, 1}, theme, focused == :buttons)
  end
end
