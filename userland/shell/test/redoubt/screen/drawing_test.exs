defmodule Redoubt.Screen.DrawingTest do
  # What the widgets draw: into a screen buffer, its diff read by the one decoder and drawn by the
  # encoder onto the model of a terminal, which raises at any sequence but the encoder's own.
  use ExUnit.Case, async: true

  alias Redoubt.Screen.{Widget, Widgets}
  alias Redoubt.Screen.Widget.{Dialogs, Theme}
  alias Redoubt.Term.{Buffer, Cells, Frame}
  alias Redoubt.Test.Terminal

  unless Buffer.available?() do
    @moduletag skip: "the screen buffer is beamlet's natives, which the BEAM does not have"
  end

  @plain Theme.plain()
  # Text that would drive a terminal: set the clipboard, colour what follows, reorder it, ring.
  @hostile "\e]52;c;aGk=\a\e[1;31m\u202Eevil"
  @shown "^[]52;c;aGk=^G^[[1;31m<U+202E>evil"

  defp render(cols, rows, draw) do
    buffer = Buffer.new(cols, rows)
    draw.(buffer)
    {:ok, frame} = Cells.decode(Buffer.diff(buffer))
    cols |> Terminal.new(rows) |> Terminal.feed(Frame.draw(frame))
  end

  defp k(key, mods \\ []), do: {:key, key, mods}

  defp after_keys(widget, keys),
    do: Enum.reduce(keys, widget, fn key, w -> elem(w.__struct__.key(w, key), 1) end)

  # A style as the model of the terminal holds one.
  defp model({fg, bg, bits}) do
    names = [:bold, :dim, :italic, :underlined, :slow_blink, :rapid_blink, :reversed, :hidden, :crossed_out]

    mods =
      for {name, i} <- Enum.with_index(names),
          Bitwise.band(bits, Bitwise.bsl(1, i)) != 0,
          into: MapSet.new(),
          do: name

    %{fg: fg, bg: bg, modifiers: mods}
  end

  defp style_at(t, {row, col}), do: Terminal.style(t, row, col)

  # Where `text` begins on the screen, `{row, col}` in columns.
  defp find(t, text) do
    t
    |> Terminal.lines()
    |> Enum.with_index()
    |> Enum.find_value(fn {line, row} ->
      case :binary.match(line, text) do
        {at, _} -> {row, Redoubt.Term.Width.columns(binary_part(line, 0, at))}
        :nomatch -> nil
      end
    end)
  end

  describe "input" do
    test "the text in the input's style, and the cursor a cell in the cursor's" do
      input = Widget.Input.new("abc") |> after_keys([k(:left)])
      t = render(10, 1, &Widget.Input.draw(input, &1, {0, 0, 10, 1}, @plain, true))
      assert Terminal.lines(t) == ["abc"]
      assert style_at(t, {0, 0}) == model(Theme.style(@plain, :input))
      assert style_at(t, {0, 2}) == model(Theme.style(@plain, :cursor)), "the cursor is on the c"
      assert style_at(t, {0, 5}) == model(Theme.style(@plain, :input)), "padded across"
    end

    test "without the focus, no cursor" do
      t = render(10, 1, &Widget.Input.draw(Widget.Input.new("abc"), &1, {0, 0, 10, 1}, @plain, false))
      assert style_at(t, {0, 3}) == model(Theme.style(@plain, :input))
    end

    test "a text wider than the input scrolls to keep the cursor in view" do
      input = Widget.Input.new("abcdefghij")
      t = render(5, 1, &Widget.Input.draw(input, &1, {0, 0, 5, 1}, @plain, true))
      assert Terminal.lines(t) == ["ghij"]
      assert style_at(t, {0, 4}) == model(Theme.style(@plain, :cursor))
      t = render(5, 1, &Widget.Input.draw(after_keys(input, [k(:home)]), &1, {0, 0, 5, 1}, @plain, true))
      assert Terminal.lines(t) == ["abcde"]
      assert style_at(t, {0, 0}) == model(Theme.style(@plain, :cursor))
    end

    test "control characters typed into it are drawn visibly, and the cursor stays after them" do
      input = Widget.Input.new() |> after_keys(@hostile |> String.graphemes() |> Enum.map(&k/1))
      assert Widget.Input.value(input) == @hostile
      t = render(60, 1, &Widget.Input.draw(input, &1, {0, 0, 60, 1}, @plain, true))
      assert Terminal.lines(t) == [@shown]
      assert style_at(t, {0, String.length(@shown)}) == model(Theme.style(@plain, :cursor))
    end
  end

  describe "list" do
    test "a checklist draws its marks; the selected row is reversed across the list's width" do
      list = Widget.List.new(["a", "b"], select: :many, marked: [1]) |> after_keys([k(:down)])
      t = render(12, 2, &Widget.List.draw(list, &1, {0, 0, 12, 2}, @plain, true))
      assert Terminal.lines(t) == [" [ ] a", " [x] b"]
      assert style_at(t, {1, 11}) == model(Theme.style(@plain, :selected))
      assert style_at(t, {0, 1}) == model(Theme.style(@plain, :normal))
    end

    test "a radio list draws its mark" do
      list = Widget.List.new(["a", "b"], select: :one) |> after_keys([k(" ")])
      t = render(12, 2, &Widget.List.draw(list, &1, {0, 0, 12, 2}, @plain, false))
      assert Terminal.lines(t) == [" (•) a", " ( ) b"]
    end
  end

  describe "menu bar" do
    @menus [{"File", [{"Open", :open}, :separator, {"Quit", :quit}]}, {"Edit", [{"Undo", :undo}]}]

    test "closed: the titles along the first row, each hot key in the hot key's style" do
      bar = Widget.MenuBar.new(@menus)
      t = render(30, 6, &Widget.MenuBar.draw(bar, &1, {0, 0, 30, 6}, @plain))
      assert Terminal.lines(t) == ["  File  Edit"]
      assert style_at(t, {0, 2}) == model(Theme.style(@plain, :hotkey))
      assert style_at(t, {0, 3}) == model(Theme.style(@plain, :menu))
      assert style_at(t, {0, 29}) == model(Theme.style(@plain, :menu))
    end

    test "open: its drop-down below, separators across it, the item selected" do
      bar = Widget.MenuBar.new(@menus) |> after_keys([k({:f, 10}), k(:down)])
      t = render(30, 6, &Widget.MenuBar.draw(bar, &1, {0, 0, 30, 6}, @plain))

      assert Enum.take(Terminal.lines(t), 5) == [
               "  File  Edit",
               " ┌──────┐",
               " │ Open │",
               " ├──────┤",
               " │ Quit │"
             ]

      assert style_at(t, {0, 2}) == model(Theme.style(@plain, :menu_selected))
      assert style_at(t, find(t, "Quit")) == model(Theme.style(@plain, :menu_selected))
      assert style_at(t, find(t, "Open")) == model(Theme.style(@plain, :menu))
    end

    test "a drop-down near the right edge is moved to fit" do
      bar =
        Widget.MenuBar.new([{"A", []}, {"Long", [{"Something", :s}]}]) |> after_keys([k({:f, 10}), k(:right)])

      t = render(12, 4, &Widget.MenuBar.draw(bar, &1, {0, 0, 12, 4}, @plain))
      assert Enum.at(Terminal.lines(t), 2) == "│ Something│"
    end
  end

  describe "table" do
    test "the header in its style, the rows in columns, the selected row reversed" do
      table = Widget.Table.new([["a.log", 12], ["世界", 3]], header: ["name", "size"]) |> after_keys([k(:down)])
      t = render(20, 3, &Widget.Table.draw(table, &1, {0, 0, 20, 3}, @plain, true))
      assert Terminal.lines(t) == ["name  size", "a.log 12", "世界  3"]
      assert style_at(t, {0, 0}) == model(Theme.style(@plain, :header))
      assert style_at(t, {2, 19}) == model(Theme.style(@plain, :selected))
    end

    test "a long table scrolls to keep the selection in view" do
      table = Widget.Table.new(Enum.map(1..10, &[&1])) |> after_keys([k(:end)])
      t = render(4, 3, &Widget.Table.draw(table, &1, {0, 0, 4, 3}, @plain, true))
      assert Terminal.lines(t) == ["8", "9", "10"]
    end
  end

  describe "canvas" do
    test "dots are drawn as Braille cells" do
      canvas = Widget.Canvas.new(2, 1) |> Widget.Canvas.line({0, 0}, {3, 3})
      t = render(4, 1, &Widget.Canvas.draw(canvas, &1, {1, 0, 4, 1}))
      assert Terminal.lines(t) == [" ⠑⢄"]
    end
  end

  describe "dialogs" do
    test "a confirm, centred, its title in the border, its text wrapped and its buttons below" do
      stack =
        Dialogs.push(Dialogs.new(), Dialogs.confirm(:c, "remove", "remove every file below here, for good?"))

      t = render(40, 12, &Dialogs.draw(stack, &1, {0, 0, 40, 12}, @plain))
      text = Terminal.text(t)
      assert text =~ "┌─ remove ─"
      assert text =~ "remove every file below here, for"
      assert text =~ "good?"
      assert text =~ "< Yes >  < No >"
      assert style_at(t, find(t, "< Yes >")) == model(Theme.style(@plain, :button_focused))
      assert style_at(t, find(t, "< No >")) == model(Theme.style(@plain, :button))
    end

    test "the stack is laid out again at a new size, the top dialog last" do
      stack =
        Dialogs.new()
        |> Dialogs.push(Dialogs.message(:m, "under", "the one below"))
        |> Dialogs.push(Dialogs.prompt(:p, "name", "a name?", "x"))

      for {cols, rows} <- [{60, 16}, {36, 9}] do
        t = render(cols, rows, &Dialogs.draw(stack, &1, {0, 0, cols, rows}, @plain))
        assert Terminal.text(t) =~ "< OK >  < Cancel >"
        assert Terminal.text(t) =~ "a name?"
        {row, col} = find(t, "┌─ name ─")
        assert row >= 0 and col + 34 <= cols, "the prompt fits the screen"
        assert style_at(t, find(t, "x")) == model(Theme.style(@plain, :input))
      end
    end

    test "a screen too small for a dialog draws none of it" do
      stack = Dialogs.push(Dialogs.new(), Dialogs.message(:m, "t", "text"))
      t = render(20, 3, &Dialogs.draw(stack, &1, {0, 0, 20, 3}, @plain))
      assert Terminal.text(t) |> String.trim() == ""
    end
  end

  describe "completion pop-up" do
    test "below the cell when there is room, above it when there is not" do
      list = Widget.List.new(["alpha", "beta"])
      t = render(20, 10, &Widgets.popup(&1, {3, 1}, list, {0, 0, 20, 10}, @plain))
      assert find(t, "alpha") == {3, 5}
      t = render(20, 10, &Widgets.popup(&1, {3, 8}, list, {0, 0, 20, 10}, @plain))
      assert find(t, "alpha") == {5, 5}
      assert style_at(t, find(t, "alpha")) == model(Theme.style(@plain, :menu_selected))
    end
  end

  describe "hostile text and styles" do
    test "every widget draws a control character visibly, in its role's style and no other" do
      theme = Theme.qbasic()
      bar = Widget.MenuBar.new([{@hostile, [{@hostile, :x}]}])

      draws = [
        {&Widget.List.draw(Widget.List.new([@hostile]), &1, {0, 0, 80, 1}, theme, false), :normal},
        {&Widget.Table.draw(Widget.Table.new([[@hostile]]), &1, {0, 0, 80, 1}, theme, false), :normal},
        {&Widget.Input.draw(Widget.Input.new(@hostile), &1, {0, 0, 80, 1}, theme, false), :input},
        {&Widget.Buttons.draw(Widget.Buttons.new([{@hostile, 1}]), &1, {0, 0, 80, 1}, theme, false), :button},
        {&Widget.MenuBar.draw(after_keys(bar, [k({:f, 10})]), &1, {0, 0, 80, 4}, theme), :menu_selected},
        {&Widgets.status(&1, {0, 0, 80, 1}, @hostile, Theme.style(theme, :status)), :status}
      ]

      for {draw, role} <- draws do
        t = render(80, 4, draw)
        assert Terminal.text(t) =~ @shown
        at = find(t, "evil")
        assert style_at(t, at) == model(Theme.style(theme, role)), "#{role}: the role's style, not red"
      end
    end

    test "a dialog's title and text are drawn visibly in the dialog's style" do
      theme = Theme.menuconfig()
      stack = Dialogs.push(Dialogs.new(), Dialogs.message(:m, @hostile, @hostile))
      t = render(80, 12, &Dialogs.draw(stack, &1, {0, 0, 80, 12}, theme))
      assert Terminal.text(t) |> String.split(@shown) |> length() == 3, "the title and the text"
      for line <- Terminal.lines(t), line =~ "evil", do: refute(line =~ "\e")
      assert style_at(t, find(t, "evil")) == model(Theme.style(theme, :dialog))
    end
  end
end
