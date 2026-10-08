defmodule Redoubt.Screen.Widget.Theme do
  @moduledoc """
  A theme is a map from roles to styles (docs/userland/shell.md, "Widgets, focus and themes"):
  widgets draw each part in the style of its role, and never take a style from what they show.

  The roles:
  - `:normal`, `:selected`, `:header`, `:status`: a screen's text, the selected row of a list or
    table, a table's header and the status line;
  - `:border`, `:shadow`: a box's border and the shadow beside it;
  - `:menu`, `:menu_selected`, `:hotkey`: the menu bar and its drop-downs, the open menu or the
    item under the selection, and the letter that opens a menu;
  - `:dialog`, `:button`, `:button_focused`, `:input`, `:cursor`: a dialog's body, its buttons, a
    text input and the cell its cursor is on.

  Three are built: `plain` (the terminal's own colours, reversed for what is selected; the
  default), `qbasic` (QBasic's blue) and `menuconfig` (the kernel's `menuconfig`).
  """

  alias Redoubt.Term.Buffer

  @type role ::
          :normal
          | :selected
          | :header
          | :status
          | :border
          | :shadow
          | :menu
          | :menu_selected
          | :hotkey
          | :dialog
          | :button
          | :button_focused
          | :input
          | :cursor
  @type t :: %{role() => Buffer.style()}

  @names [:plain, :qbasic, :menuconfig]

  @doc "The theme of one of the names `:plain`, `:qbasic` and `:menuconfig`."
  @spec get(atom()) :: t()
  def get(name) when name in @names, do: apply(__MODULE__, name, [])

  def get(name),
    do: raise(ArgumentError, "no theme #{inspect(name)}: the themes are #{inspect(@names)}")

  @doc "The style of `role` in `theme`."
  @spec style(t(), role()) :: Buffer.style()
  def style(theme, role), do: Map.fetch!(theme, role)

  @doc "The terminal's own colours; what is selected or focused is reversed."
  @spec plain() :: t()
  def plain do
    normal = Buffer.plain()
    reversed = Buffer.style(reversed: true)

    %{
      normal: normal,
      selected: reversed,
      header: Buffer.style(bold: true),
      status: reversed,
      border: normal,
      shadow: Buffer.style(bg: {:indexed, 0}),
      menu: reversed,
      menu_selected: normal,
      hotkey: Buffer.style(reversed: true, bold: true, underlined: true),
      dialog: normal,
      button: normal,
      button_focused: reversed,
      input: Buffer.style(underlined: true),
      cursor: reversed
    }
  end

  @doc "QBasic's: white on blue, menus and dialogs black on grey."
  @spec qbasic() :: t()
  def qbasic do
    on_blue = Buffer.style(fg: {:indexed, 7}, bg: {:indexed, 4})
    on_grey = Buffer.style(fg: {:indexed, 0}, bg: {:indexed, 7})
    inverse = Buffer.style(fg: {:indexed, 7}, bg: {:indexed, 0})

    %{
      normal: on_blue,
      selected: on_grey,
      header: Buffer.style(fg: {:indexed, 15}, bg: {:indexed, 4}, bold: true),
      status: Buffer.style(fg: {:indexed, 15}, bg: {:indexed, 3}),
      border: on_blue,
      shadow: Buffer.style(bg: {:indexed, 0}),
      menu: on_grey,
      menu_selected: inverse,
      hotkey: Buffer.style(fg: {:indexed, 15}, bg: {:indexed, 7}, bold: true),
      dialog: on_grey,
      button: on_grey,
      button_focused: inverse,
      input: Buffer.style(fg: {:indexed, 7}, bg: {:indexed, 0}),
      cursor: on_grey
    }
  end

  @doc "`menuconfig`'s: grey boxes on blue, the selection in blue."
  @spec menuconfig() :: t()
  def menuconfig do
    on_grey = Buffer.style(fg: {:indexed, 0}, bg: {:indexed, 7})
    selected = Buffer.style(fg: {:indexed, 15}, bg: {:indexed, 4}, bold: true)

    %{
      normal: on_grey,
      selected: selected,
      header: Buffer.style(fg: {:indexed, 0}, bg: {:indexed, 7}, bold: true),
      status: Buffer.style(fg: {:indexed, 15}, bg: {:indexed, 4}),
      border: Buffer.style(fg: {:indexed, 15}, bg: {:indexed, 7}, bold: true),
      shadow: Buffer.style(bg: {:indexed, 0}),
      menu: on_grey,
      menu_selected: selected,
      hotkey: Buffer.style(fg: {:indexed, 1}, bg: {:indexed, 7}, bold: true),
      dialog: on_grey,
      button: on_grey,
      button_focused: selected,
      input: Buffer.style(fg: {:indexed, 15}, bg: {:indexed, 4}),
      cursor: on_grey
    }
  end
end
