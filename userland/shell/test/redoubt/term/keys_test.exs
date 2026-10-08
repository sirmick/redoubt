defmodule Redoubt.Term.KeysTest do
  use ExUnit.Case, async: true

  alias Redoubt.Term.Keys

  defp keys(bytes) do
    {keys, ""} = Keys.decode(bytes)
    Enum.map(keys, fn {:key, k, mods} -> {k, mods} end)
  end

  test "printable text is a key a grapheme, wide and joined ones included" do
    assert keys("aZ世e\u0301") == [{"a", []}, {"Z", []}, {"世", []}, {"e\u0301", []}]
  end

  test "Enter, Tab, Backspace and the control letters" do
    assert keys("\r\n\t\x7F\b") == [
             {:enter, []},
             {:enter, []},
             {:tab, []},
             {:backspace, []},
             {:backspace, []}
           ]

    assert keys("\x01\x03\x1A\x00\x1C") == [
             {"a", [:ctrl]},
             {"c", [:ctrl]},
             {"z", [:ctrl]},
             {" ", [:ctrl]},
             {"\\", [:ctrl]}
           ]
  end

  test "cursor keys in their CSI and SS3 spellings, and xterm's modifier forms" do
    assert keys("\e[A\e[B\e[C\e[D\e[H\e[F") == [
             {:up, []},
             {:down, []},
             {:right, []},
             {:left, []},
             {:home, []},
             {:end, []}
           ]

    assert keys("\eOA\eOD\eOH") == [{:up, []}, {:left, []}, {:home, []}]

    assert keys("\e[1;5C\e[1;2A\e[1;3D\e[1;8B") ==
             [{:right, [:ctrl]}, {:up, [:shift]}, {:left, [:alt]}, {:down, [:shift, :alt, :ctrl]}]
  end

  test "editing and function keys: CSI n ~, SS3 F1-F4, the Linux console's F1-F5, Shift+Tab" do
    assert keys("\e[2~\e[3~\e[5~\e[6~\e[1~\e[4~\e[3;5~") ==
             [
               {:insert, []},
               {:delete, []},
               {:page_up, []},
               {:page_down, []},
               {:home, []},
               {:end, []},
               {:delete, [:ctrl]}
             ]

    assert keys("\eOP\eOS\e[15~\e[24~\e[[A\e[[E\e[Z") ==
             [
               {{:f, 1}, []},
               {{:f, 4}, []},
               {{:f, 5}, []},
               {{:f, 12}, []},
               {{:f, 1}, []},
               {{:f, 5}, []},
               {:tab, [:shift]}
             ]
  end

  test "Alt is ESC before a key" do
    assert keys("\ef\e\x7F\e\e[A") == [{"f", [:alt]}, {:backspace, [:alt]}, {:up, [:alt]}]
  end

  test "a sequence it does not know is dropped whole, not read as its characters" do
    assert keys("a\e[?1049hb\e]0;x") == [{"a", []}, {"b", []}, {"]", [:alt]}, {"0", []}, {";", []}, {"x", []}]
    assert keys("\e[99~c\eOzd") == [{"c", []}, {"d", []}]
  end

  test "a byte that is not UTF-8 is not a key" do
    assert keys(<<"a", 0xFF, "b">>) == [{"a", []}, {"b", []}]
  end

  test "what may begin a longer key waits as pending, and needs no clock" do
    assert Keys.decode("ab\e") == {[{:key, "a", []}, {:key, "b", []}], "\e"}
    assert Keys.decode("\e[1;5") == {[], "\e[1;5"}
    assert Keys.decode("\eO") == {[], "\eO"}
    assert Keys.decode("\e[[") == {[], "\e[["}
    assert Keys.decode(<<"x", 0xE4, 0xB8>>) == {[{:key, "x", []}], <<0xE4, 0xB8>>}
    # The rest arriving completes it.
    assert Keys.decode("\e" <> "[A") == {[{:key, :up, []}], ""}
  end

  test "flushed, a lone ESC is Esc, and an unfinished sequence is Esc and what followed" do
    assert Keys.flush("\e") == [{:key, :esc, []}]
    assert Keys.flush("\e[") == [{:key, :esc, []}, {:key, "[", []}]
    assert Keys.flush(<<0xE4, 0xB8>>) == []
    assert Keys.flush("") == []
  end
end
