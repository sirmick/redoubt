defmodule Redoubt.Screen.WidgetTest do
  # What each widget does with a key: plain data in, plain data out, on the BEAM and on beamlet.
  use ExUnit.Case, async: true

  import Bitwise

  alias Redoubt.Screen.{Widget, Widgets}
  alias Redoubt.Screen.Widget.{Dialogs, Focus, Theme}

  defp k(key, mods \\ []), do: {:key, key, mods}

  # Each key in turn, the widget kept from each `{:cont, w}`; the last answer.
  defp keys(widget, keys) do
    Enum.reduce(keys, {:cont, widget}, fn key, {:cont, w} -> widget.__struct__.key(w, key) end)
  end

  defp typed(text), do: text |> String.graphemes() |> Enum.map(&k/1)

  describe "list" do
    test "the arrows, Page Up and Down, Home and End move; Enter chooses the selected item" do
      list = Widget.List.new(Enum.to_list(1..30), page: 5)
      assert {:done, 2, _} = keys(list, [k(:down), k(:enter)])
      assert {:done, 6, _} = keys(list, [k(:page_down), k(:enter)])
      assert {:done, 30, _} = keys(list, [k(:end), k(:down), k(:enter)])
      assert {:done, 1, _} = keys(list, [k(:end), k(:home), k(:up), k(:enter)])
      assert {:done, 25, _} = keys(list, [k(:end), k(:page_up), k(:enter)])
    end

    test "a key it does not take is passed on, and an empty list takes none" do
      assert Widget.List.key(Widget.List.new([:a]), k(:esc)) == :pass
      assert Widget.List.key(Widget.List.new([:a]), k(:down, [:ctrl])) == :pass
      assert Widget.List.key(Widget.List.new([]), k(:enter)) == :pass
    end

    test "a radio list chooses the marked item, and Space moves the mark" do
      radio = Widget.List.new([:a, :b, :c], select: :one)
      assert {:done, :b, _} = keys(radio, [k(:down), k(" "), k(:down), k(:enter)])
      assert {:done, :c, _} = keys(radio, [k(" "), k(:down), k(:down), k(" "), k(:up), k(:enter)])
      assert {:done, :a, _} = keys(radio, [k(:enter)]), "with none marked, the selected one"
    end

    test "a checklist chooses the marked items, in the list's order; Space marks and unmarks" do
      check = Widget.List.new([:a, :b, :c, :d], select: :many, marked: [3])
      assert {:done, [:a, :c, :d], _} = keys(check, [k(:down), k(:down), k(" "), k(:home), k(" "), k(:enter)])
      assert {:done, [], _} = keys(check, [k(:end), k(" "), k(:enter)])
    end

    test "Space is passed on by a plain list" do
      assert Widget.List.key(Widget.List.new([:a]), k(" ")) == :pass
    end
  end

  describe "pick" do
    alias Redoubt.Screen.Pick

    test "a key counts with any modifiers held: Esc leaves, the arrows move, Enter chooses" do
      state = Pick.init({[:a, :b], nil})
      assert Pick.update(k(:esc, [:alt]), state) == {:halt, nil}
      assert Pick.update(k(:esc), state) == {:halt, nil}
      {:cont, state} = Pick.update(k(:down, [:shift]), state)
      assert Pick.update(k(:enter, [:ctrl]), state) == {:halt, :b}
    end
  end

  describe "input" do
    test "what is typed is inserted at the cursor, which the keys move" do
      assert {:done, "hello", _} =
               keys(Widget.Input.new(), typed("hllo") ++ [k(:home), k(:right), k("e"), k(:enter)])

      assert {:done, "ab", _} =
               keys(Widget.Input.new("b"), [k("a", [:ctrl]), k("a"), k("e", [:ctrl]), k(:enter)])

      assert {:done, "Ab", _} = keys(Widget.Input.new("b"), [k(:home), k("A", [:shift]), k(:enter)])
    end

    test "Backspace, Delete, Ctrl+U and Ctrl+K remove" do
      input = Widget.Input.new("abcdef")
      assert {:done, "abcde", _} = keys(input, [k(:backspace), k(:enter)])
      assert {:done, "abcef", _} = keys(input, [k(:left), k(:left), k(:left), k(:delete), k(:enter)])
      assert {:done, "def", _} = keys(input, [k(:left), k(:left), k(:left), k("u", [:ctrl]), k(:enter)])
      assert {:done, "abc", _} = keys(input, [k(:left), k(:left), k(:left), k("k", [:ctrl]), k(:enter)])
      assert {:done, "", _} = keys(Widget.Input.new(), [k(:backspace), k(:delete), k(:left), k(:enter)])
    end

    test "Tab, Esc and Alt or Ctrl with another key are passed on, never typed" do
      input = Widget.Input.new()

      for key <- [k(:tab), k(:esc), k("x", [:alt]), k("x", [:ctrl]), k(:up)],
          do: assert(Widget.Input.key(input, key) == :pass)
    end
  end

  describe "buttons" do
    test "Left and Right move along the row; Enter or Space presses" do
      row = Widget.Buttons.new([{"Yes", true}, {"No", false}])
      assert {:done, true, _} = keys(row, [k(:enter)])
      assert {:done, false, _} = keys(row, [k(:right), k(:right), k(" ")])
      assert {:done, true, _} = keys(row, [k(:right), k(:left), k(:left), k(:enter)])
      assert Widget.Buttons.key(row, k(:tab)) == :pass
    end

    test "the row's width is its buttons, two apart" do
      assert Widget.Buttons.width(Widget.Buttons.new([{"OK", :ok}, {"Cancel", nil}])) == 6 + 10 + 2
    end
  end

  describe "menu bar" do
    @menus [{"File", [{"Open", :open}, :separator, {"Quit", :quit}]}, {"Edit", [{"Undo", :undo}]}]

    test "closed, it takes only F10 and Alt with a menu's hot key" do
      bar = Widget.MenuBar.new(@menus)
      assert Widget.MenuBar.key(bar, k(:down)) == :pass
      assert Widget.MenuBar.key(bar, k("f")) == :pass
      assert Widget.MenuBar.key(bar, k("x", [:alt])) == :pass
      assert {:cont, %{open: 0}} = Widget.MenuBar.key(bar, k({:f, 10}))
      assert {:cont, %{open: 1}} = Widget.MenuBar.key(bar, k("e", [:alt]))
      assert {:cont, %{open: 1}} = Widget.MenuBar.key(bar, k("E", [:alt]))
    end

    test "open, the arrows move over menus and items, skipping separators, and Enter chooses" do
      bar = Widget.MenuBar.new(@menus)
      assert {:done, :open, %{open: nil}} = keys(bar, [k({:f, 10}), k(:enter)])
      assert {:done, :quit, _} = keys(bar, [k({:f, 10}), k(:down), k(:enter)])
      assert {:done, :quit, _} = keys(bar, [k({:f, 10}), k(:down), k(:down), k(:enter)])
      assert {:done, :open, _} = keys(bar, [k({:f, 10}), k(:down), k(:up), k(:enter)])
      assert {:done, :undo, _} = keys(bar, [k({:f, 10}), k(:right), k(:enter)])
      assert {:done, :undo, _} = keys(bar, [k({:f, 10}), k(:left), k(:enter)]), "round the bar"
    end

    test "open, it is modal: Esc closes it, and it keeps every other key" do
      bar = Widget.MenuBar.new(@menus)
      assert {:cont, %{open: nil} = closed} = keys(bar, [k({:f, 10}), k("q"), k(:tab), k(:esc)])
      refute Widget.MenuBar.open?(closed)
    end
  end

  describe "table" do
    test "the arrows move a selection, and Enter gives the row as it was given" do
      table = Widget.Table.new([{"a", 1}, {"b", 2}, {"c", 3}], header: ["name", "n"])
      assert {:done, {"b", 2}, _} = keys(table, [k(:down), k(:enter)])
      assert {:done, {"c", 3}, _} = keys(table, [k(:end), k(:enter)])
      assert Widget.Table.key(table, k(:tab)) == :pass
    end

    test "columns are as wide as their widest cell, the header's included" do
      table = Widget.Table.new([["a", 100], ["bb", 2]], header: ["name", "n"])
      assert table.widths == [4, 3]
    end
  end

  describe "canvas" do
    alias Widget.Canvas

    test "each dot of a cell sets its bit, in Braille's order" do
      dots = for x <- 0..1, y <- 0..3, do: {x, y}
      bits = for dot <- dots, do: Canvas.new(1, 1) |> Canvas.set(dot) |> Map.fetch!(:cells) |> Map.fetch!(0)
      # Dots 1, 2, 3 and 7 down the left; 4, 5, 6 and 8 down the right.
      assert bits == [0x01, 0x02, 0x04, 0x40, 0x08, 0x10, 0x20, 0x80]
      assert (Canvas.new(1, 1) |> Canvas.points(dots)).cells == %{0 => 0xFF}
    end

    test "a dot outside the canvas is not drawn, and a line sets both its ends" do
      assert (Canvas.new(2, 1) |> Canvas.points([{-1, 0}, {4, 0}, {0, 4}])).cells == %{}
      assert Canvas.size(Canvas.new(3, 2)) == {6, 8}
      line = Canvas.new(2, 1) |> Canvas.line({0, 0}, {3, 3})
      assert line.cells == %{0 => 0x01 ||| 0x10, 1 => 0x04 ||| 0x80}
    end
  end

  describe "focus" do
    test "Tab and Shift+Tab move round the ring; other keys go to the focused widget" do
      widgets = %{name: Widget.Input.new(), ok: Widget.Buttons.new([{"OK", :ok}])}
      focus = Focus.new([:name, :ok])
      assert Focus.focused(focus) == :name
      {:cont, focus, widgets} = Focus.key(focus, widgets, k("x"))
      assert Widget.Input.value(widgets.name) == "x"
      {:cont, focus, widgets} = Focus.key(focus, widgets, k(:tab))
      assert Focus.focused(focus) == :ok
      assert {:done, :ok, :ok, _, _} = Focus.key(focus, widgets, k(:enter))
      {:cont, focus, _} = Focus.key(focus, widgets, k(:tab, [:shift]))
      assert Focus.focused(focus) == :name
      {:cont, focus, _} = Focus.key(focus, widgets, k(:tab, [:shift]))
      assert Focus.focused(focus) == :ok
      assert Focus.key(focus, widgets, k(:up)) == :pass
    end
  end

  describe "dialogs" do
    test "with none open every key is passed to the screen" do
      refute Dialogs.open?(Dialogs.new())
      assert Dialogs.key(Dialogs.new(), k(:enter)) == :pass
    end

    test "keys go to the top dialog, and one that ends leaves the one under it" do
      stack =
        Dialogs.new()
        |> Dialogs.push(Dialogs.message(:under, "a", "b"))
        |> Dialogs.push(Dialogs.confirm(:top, "c", "d"))

      {:cont, stack} = Dialogs.key(stack, k(:right))
      assert {:closed, :top, false, stack} = Dialogs.key(stack, k(:enter))
      assert Dialogs.open?(stack)
      assert {:closed, :under, :ok, stack} = Dialogs.key(stack, k(:enter))
      refute Dialogs.open?(stack)
    end

    test "a prompt gives the text typed, by Enter in its input or its OK, and nil by Cancel or Esc" do
      prompt = Dialogs.new() |> Dialogs.push(Dialogs.prompt(:p, "name", "a name?", "x"))
      step = fn stack, key -> Dialogs.key(stack, key) end
      run = fn keys -> Enum.reduce(keys, {:cont, prompt}, fn key, {:cont, s} -> step.(s, key) end) end
      assert {:closed, :p, "xyz", _} = run.(typed("yz") ++ [k(:enter)])
      assert {:closed, :p, "xy", _} = run.([k("y"), k(:tab), k(:enter)])
      assert {:closed, :p, nil, _} = run.([k("y"), k(:tab), k(:right), k(:enter)])
      assert {:closed, :p, nil, _} = run.([k("y"), k(:esc)])
    end

    test "a modal dialog keeps the keys it does not take" do
      stack = Dialogs.new() |> Dialogs.push(Dialogs.message(:m, "a", "b"))
      assert {:cont, ^stack} = Dialogs.key(stack, k(:up))
    end
  end

  describe "themes" do
    test "every theme has a style for every role, and only the built names are themes" do
      roles = Theme.plain() |> Map.keys() |> Enum.sort()

      for name <- [:plain, :qbasic, :menuconfig],
          do: assert(name |> Theme.get() |> Map.keys() |> Enum.sort() == roles)

      assert_raise ArgumentError, fn -> Theme.get(:"\e[31m") end
      assert_raise ArgumentError, fn -> Theme.get("plain") end
    end
  end

  describe "wrap" do
    test "text breaks at spaces, inside a word too long for a row, and at newlines; made visible" do
      assert Widgets.wrap("one two three", 7) == ["one two", "three"]
      assert Widgets.wrap("abcdefghij", 4) == ["abcd", "efgh", "ij"]
      assert Widgets.wrap("a\nb", 10) == ["a", "b"]
      assert Widgets.wrap("\e[2J", 10) == ["^[[2J"]
    end
  end
end
