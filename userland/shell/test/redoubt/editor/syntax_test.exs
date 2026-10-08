defmodule Redoubt.Editor.SyntaxTest do
  # The editor's highlighting: each language's roles, the states that carry a string or a comment
  # from line to line, and that a file's text can choose nothing but a role. On both VMs.
  use ExUnit.Case, async: true

  alias Redoubt.Editor.{Syntax, View}
  alias Redoubt.Screen.Widget.Theme

  @languages [Syntax.Ex, Syntax.Erl, Syntax.Rust, Syntax.Markdown, Syntax.Toml, Syntax.Json]
  @roles [:normal, :keyword, :string, :comment, :number, :constant, :heading]

  # The lines, highlighted one after another, as `<role:text>` around all but plain text.
  defp marked(language, lines) do
    {marked, _state} =
      Enum.map_reduce(lines, Syntax.start(), fn line, state ->
        {pieces, state} = Syntax.line(language, line, state)
        {Enum.map_join(pieces, fn {t, r} -> if r == :normal, do: t, else: "<#{r}:#{t}>" end), state}
      end)

    marked
  end

  test "a language is chosen by the file's extension; any other file is plain text" do
    assert Syntax.language("/a/b.ex") == Syntax.Ex and Syntax.language("x.EXS") == Syntax.Ex
    assert Syntax.language("m.erl") == Syntax.Erl and Syntax.language("main.rs") == Syntax.Rust
    assert Syntax.language("README.md") == Syntax.Markdown and Syntax.language("Cargo.toml") == Syntax.Toml
    assert Syntax.language("a.json") == Syntax.Json
    for path <- ["notes.txt", "Makefile", "a.ex.bak", ".ex"], do: assert(Syntax.language(path) == nil, path)
    assert Syntax.line(nil, "def x", :code) == {[{"def x", :normal}], :code}
  end

  test "Elixir: keywords, aliases, atoms and keys, strings, characters, comments and heredocs" do
    assert marked(Syntax.Ex, [
             ~S|defmodule Foo.Bar do # hi|,
             ~S|  x(do: :ok, s: "a\"b" <> ?" <> 'c', n: 1_000)|,
             ~S|  @doc """|,
             ~S|  "inside" # not a comment|,
             ~S|  """|
           ]) == [
             ~S|<keyword:defmodule> <constant:Foo>.<constant:Bar> <keyword:do> <comment:# hi>|,
             ~S|  x(<constant:do:> <constant::ok>, <constant:s:> <string:"a\"b"> <> <constant:?"> <> <string:'c'>, <constant:n:> <number:1_000>)|,
             ~S|  @doc <string:""">|,
             ~S|<string:  "inside" # not a comment>|,
             ~S|<string:  """>|
           ]
  end

  test "Erlang: keywords, quoted atoms, characters and comments; a variable is plain" do
    assert marked(Syntax.Erl, [~S|f(X) when X > $a -> 'Q'; f(_) -> "s". % c|]) ==
             [
               ~S|f(X) <keyword:when> X > <constant:$a> -> <constant:'Q'>; f(_) -> <string:"s">. <comment:% c>|
             ]
  end

  test "Rust: block comments nest and run on; strings run on; a lifetime's quote is plain" do
    assert marked(Syntax.Rust, [
             "fn f<'a>() { /* a /* b */ still",
             "end */ let x: Vec<u8> = 3; // c",
             ~S|let s = "multi|,
             ~S|line";|
           ]) == [
             "<keyword:fn> f<'a>() { <comment:/* a /* b */ still>",
             "<comment:end */> <keyword:let> x: <constant:Vec><u8> = <number:3>; <comment:// c>",
             ~S|<keyword:let> s = <string:"multi>|,
             ~S|<string:line">;|
           ]
  end

  test "Markdown: headings, fenced blocks and code spans" do
    assert marked(Syntax.Markdown, [
             "# Title",
             "a `b` c `open",
             "```elixir",
             "# not a heading",
             "```",
             "####### no"
           ]) ==
             [
               "<heading:# Title>",
               "a <string:`b`> c `open",
               "<string:```elixir>",
               "<string:# not a heading>",
               "<string:```>",
               "####### no"
             ]
  end

  test "TOML and JSON: tables, strings, raw strings over lines, constants and numbers" do
    assert marked(Syntax.Toml, ["[package]", ~S|name = "a" # c|, "on = true", "s = '''", "raw \\", "'''"]) ==
             [
               "<heading:[package]>",
               ~S|name = <string:"a"> <comment:# c>|,
               "on = <constant:true>",
               "s = <string:'''>",
               "<string:raw \\>",
               "<string:'''>"
             ]

    assert marked(Syntax.Json, [~S|{"a": [1, true, null, "x\"y"]}|]) ==
             [~S|{<string:"a">: [<number:1>, <constant:true>, <constant:null>, <string:"x\"y">]}|]
  end

  @hostile [
    "\e[31mred\e[0m \"\e]0;title\a\" # \e[2J",
    "#{<<0x202E::utf8>>}evil \"#{<<0x202E::utf8>>}\" /* \u0000 */",
    "<FF><FE> 'x' `y` ?\\ $",
    "\"unclosed \\",
    "é \"é\" 世界 // 世"
  ]

  test "a line's pieces are its own bytes, in order, each in one of the roles, in every language" do
    for language <- @languages, line <- @hostile, state <- [:code | states(language)] do
      {pieces, _state} = Syntax.line(language, line, state)
      assert Enum.map_join(pieces, &elem(&1, 0)) == line, inspect({language, line, state})
      assert Enum.all?(pieces, fn {_text, role} -> role in @roles end)
    end
  end

  # The states a language leaves, from lines that open each kind of string or comment.
  defp states(language) do
    for line <- [~S|"|, ~S|"""|, "'''", "/*", "```"], uniq: true do
      line |> then(&Syntax.line(language, &1, :code)) |> elem(1)
    end
  end

  test "a long line costs time in its length: a 1 MiB line of spaces, whole and as drawn" do
    line = String.duplicate(" ", 1024 * 1024)

    # The scanner, on the whole line: one run of plain text, in linear time.
    {us, {pieces, :code}} = :timer.tc(fn -> Syntax.Ex.line(line, :code) end)
    assert pieces == [{line, :normal}]
    assert us < 20_000_000, "#{div(us, 1000)} ms for 1 MiB"

    # As drawn: only the line's first bytes are read, the rest is plain.
    {us, {pieces, :code}} = :timer.tc(fn -> Syntax.line(Syntax.Rust, "// " <> line, :code) end)
    assert [{comment, :comment}, {rest, :normal}] = pieces
    assert byte_size(comment) == Syntax.scanned() and comment <> rest == "// " <> line
    assert us < 1_000_000, "#{div(us, 1000)} ms for a line as drawn"
  end

  test "what a file holds is drawn visibly in its role's style: an escape in a string stays text" do
    line = ~S|x = "| <> "\e[31m\e]0;t\a" <> ~S|"|
    {pieces, :code} = Syntax.line(Syntax.Ex, line, :code)

    assert View.runs(line, 0, 40, nil, nil, false, pieces) ==
             [{0, "x = ", :normal}, {4, ~S|"^[[31m^[]0;t^G"|, :string}]

    # The cursor and the selection are drawn over the highlighting.
    assert View.runs("def", 0, 40, 1..2, 0, false, [{"def", :keyword}]) == [
             {0, "d", :cursor},
             {1, "ef", :selected}
           ]
  end

  test "every theme has a style for every role of the highlighting" do
    for name <- [:plain, :qbasic, :menuconfig], role <- @roles do
      assert {_fg, _bg, _attributes} = Theme.style(Theme.get(name), role)
    end
  end
end
