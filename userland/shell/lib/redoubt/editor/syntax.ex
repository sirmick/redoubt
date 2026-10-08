defmodule Redoubt.Editor.Syntax do
  @moduledoc """
  The editor's syntax highlighting (docs/userland/shell.md, "The editor"): a line cut into
  pieces, each with a role of the theme. A language is chosen by the file's extension, and its
  module is loaded only when a file of it opens.

  A language scans one line at a time, from the state the line before left (inside a string or
  a comment that runs on, or not), and gives the line's pieces and the state it leaves. The
  pieces are the line's own bytes, in order, so a role is all a file's text can choose, and only
  among the roles below; what each looks like is the theme's.

  Most languages are a table read by `scan/3`: comment and string delimiters, keywords and
  constants. Markdown is lines of its own kind (`Redoubt.Editor.Syntax.Markdown`).
  """

  @typedoc "What a piece of a line is: each is a role of the theme."
  @type role :: :normal | :keyword | :string | :comment | :number | :constant | :heading

  @typedoc "A piece of a line, and its role."
  @type piece :: {String.t(), role()}

  @typedoc "Where a line starts: in code, or inside a string or a comment the line before opened."
  @type state :: :code | term()

  @doc "A line's pieces, from the state the line before left, and the state this one leaves."
  @callback line(String.t(), state()) :: {[piece()], state()}

  # The bytes of a line the highlighting reads.
  @scanned 4096

  @languages %{
    ".ex" => Redoubt.Editor.Syntax.Ex,
    ".exs" => Redoubt.Editor.Syntax.Ex,
    ".erl" => Redoubt.Editor.Syntax.Erl,
    ".hrl" => Redoubt.Editor.Syntax.Erl,
    ".rs" => Redoubt.Editor.Syntax.Rust,
    ".md" => Redoubt.Editor.Syntax.Markdown,
    ".toml" => Redoubt.Editor.Syntax.Toml,
    ".json" => Redoubt.Editor.Syntax.Json
  }

  @doc "The language of the file at `path`, by its extension, or `nil` for plain text."
  @spec language(Path.t()) :: module() | nil
  def language(path), do: Map.get(@languages, path |> Path.extname() |> String.downcase())

  @doc "The state the first line starts in."
  @spec start() :: state()
  def start, do: :code

  @doc """
  `line`'s pieces in `language` (`nil` is plain text), and the state it leaves. Only the line's
  first `scanned/0` bytes are read; the rest is plain, and the state is where they leave off.
  """
  @spec line(module() | nil, String.t(), state()) :: {[piece()], state()}
  def line(nil, text, state), do: {[{text, :normal}], state}

  def line(language, text, state) when byte_size(text) > @scanned do
    <<head::binary-size(@scanned), rest::binary>> = text
    {pieces, state} = language.line(head, state)
    {pieces ++ [{rest, :normal}], state}
  end

  def line(language, text, state), do: language.line(text, state)

  @doc "The bytes of a line the highlighting reads, at most: so a window costs a bounded scan."
  @spec scanned() :: pos_integer()
  def scanned, do: @scanned

  @doc """
  The state after `lines`, in order, from `state`: what the line after them starts in.
  """
  @spec after_lines(module() | nil, [String.t()], state()) :: state()
  def after_lines(nil, _lines, state), do: state

  def after_lines(language, lines, state),
    do: Enum.reduce(lines, state, fn text, state -> language |> line(text, state) |> elem(1) end)

  # ---- the scanner a table drives ----

  @typedoc """
  A language as a table:
  - `comment`: what starts a comment to the line's end;
  - `block`: a comment's start and end, and whether they nest, or `nil`;
  - `strings`: each string's start and end, its role, whether it runs on past a line, and
    whether a backslash escapes the next character; the first that matches is taken, so a
    longer start comes before a shorter;
  - `keywords`, `constants`: words of those roles;
  - `capitals`: the role of a word that starts with a capital, or `:normal`;
  - `atoms`: whether `:word` and `word:` are constants, as in Elixir;
  - `char`: what starts a character literal of one character (`?` in Elixir), or `nil`;
  - `heading`: whether a line that starts with `[` is a heading, as a table is in TOML.
  """
  @type table :: %{
          comment: [String.t()],
          block: {String.t(), String.t(), boolean()} | nil,
          strings: [%{open: String.t(), close: String.t(), role: role(), lines: boolean(), escape: boolean()}],
          keywords: MapSet.t(),
          constants: MapSet.t(),
          capitals: role(),
          atoms: boolean(),
          char: String.t() | nil,
          heading: boolean()
        }

  @doc "A language's table from its parts, by the names of `t:table/0`; a part not given is empty."
  @spec table(keyword()) :: table()
  def table(opts) do
    %{
      comment: Keyword.get(opts, :comment, []),
      block: Keyword.get(opts, :block),
      strings: Enum.map(Keyword.get(opts, :strings, []), &string/1),
      keywords: MapSet.new(Keyword.get(opts, :keywords, [])),
      constants: MapSet.new(Keyword.get(opts, :constants, [])),
      capitals: Keyword.get(opts, :capitals, :normal),
      atoms: Keyword.get(opts, :atoms, false),
      char: Keyword.get(opts, :char),
      heading: Keyword.get(opts, :heading, false)
    }
  end

  defp string({open, close, role, lines, escape}),
    do: %{open: open, close: close, role: role, lines: lines, escape: escape}

  @doc "`text`'s pieces as `table` reads them, from `state`, and the state the line leaves."
  @spec scan(table(), String.t(), state()) :: {[piece()], state()}
  def scan(table, text, state) do
    {spans, state} =
      if table.heading and state == :code and String.starts_with?(String.trim_leading(text), "["),
        do: {[{byte_size(text), :heading}], :code},
        else: run(table, text, state, [])

    {spans |> Enum.reverse() |> merge([]) |> pieces(text, 0, []), state}
  end

  # The scan gives each piece as its length and role, last first: neighbours of one role are one
  # piece, and each is cut from the line once, so a line's cost is its length.
  defp merge([{b, role} | rest], [{a, role} | acc]), do: merge(rest, [{a + b, role} | acc])
  defp merge([span | rest], acc), do: merge(rest, [span | acc])
  defp merge([], acc), do: Enum.reverse(acc)

  defp pieces([], _text, _at, acc), do: Enum.reverse(acc)

  defp pieces([{n, role} | rest], text, at, acc),
    do: pieces(rest, text, at + n, [{binary_part(text, at, n), role} | acc])

  defp run(_table, "", state, acc), do: {acc, state}

  defp run(table, text, {:string, i}, acc) do
    s = Enum.at(table.strings, i)

    case closed_at(text, s.close, s.escape, 0) do
      nil -> {[{byte_size(text), s.role} | acc], if(s.lines, do: {:string, i}, else: :code)}
      n -> cut(table, text, n, s.role, :code, acc)
    end
  end

  defp run(table, text, {:comment, depth}, acc) do
    {open, close, nest} = table.block

    case comment_end(text, open, close, nest, depth, 0) do
      {:open, depth} -> {[{byte_size(text), :comment} | acc], {:comment, depth}}
      n -> cut(table, text, n, :comment, :code, acc)
    end
  end

  defp run(table, text, :code, acc) do
    cond do
      Enum.any?(table.comment, &String.starts_with?(text, &1)) ->
        {[{byte_size(text), :comment} | acc], :code}

      table.block != nil and String.starts_with?(text, elem(table.block, 0)) ->
        cut(table, text, byte_size(elem(table.block, 0)), :comment, {:comment, 1}, acc)

      i = Enum.find_index(table.strings, &String.starts_with?(text, &1.open)) ->
        s = Enum.at(table.strings, i)
        cut(table, text, byte_size(s.open), s.role, {:string, i}, acc)

      true ->
        {n, role} = token(table, text)
        cut(table, text, n, role, :code, acc)
    end
  end

  # The first `n` bytes of `text` as one piece in `role`; the rest from `state`.
  defp cut(table, text, n, role, state, acc),
    do: run(table, binary_part(text, n, byte_size(text) - n), state, [{n, role} | acc])

  # Where the string's end is, past it, in bytes; `nil` when the line ends first.
  defp closed_at(text, close, escape, at) do
    cond do
      at >= byte_size(text) -> nil
      escape and :binary.at(text, at) == ?\\ -> closed_at(text, close, escape, at + 2)
      at?(text, at, close) -> at + byte_size(close)
      true -> closed_at(text, close, escape, at + 1)
    end
  end

  # Where the comment's end is, past it, in bytes, or `{:open, depth}` when the line ends first.
  defp comment_end(text, open, close, nest, depth, at) do
    cond do
      at >= byte_size(text) -> {:open, depth}
      at?(text, at, close) and depth == 1 -> at + byte_size(close)
      at?(text, at, close) -> comment_end(text, open, close, nest, depth - 1, at + byte_size(close))
      nest and at?(text, at, open) -> comment_end(text, open, close, nest, depth + 1, at + byte_size(open))
      true -> comment_end(text, open, close, nest, depth, at + 1)
    end
  end

  defp at?(text, at, s), do: byte_size(text) - at >= byte_size(s) and binary_part(text, at, byte_size(s)) == s

  # The token at the start of `text`, in code: its length in bytes and its role.
  defp token(_table, <<c, _::binary>> = text) when c in ?0..?9, do: {span(text, &number?/1), :number}

  defp token(%{atoms: true}, <<?:, c, _::binary>> = text) when c in ?a..?z or c in ?A..?Z or c == ?_,
    do: {1 + span(binary_part(text, 1, byte_size(text) - 1), &word?/1), :constant}

  defp token(table, <<c, _::binary>> = text) when c in ?a..?z or c in ?A..?Z or c == ?_ do
    n = span(text, &word?/1)
    word = binary_part(text, 0, n)

    cond do
      table.atoms and key?(text, n) -> {n + 1, :constant}
      MapSet.member?(table.keywords, word) -> {n, :keyword}
      MapSet.member?(table.constants, word) -> {n, :constant}
      c in ?A..?Z -> {n, table.capitals}
      true -> {n, :normal}
    end
  end

  defp token(%{char: char}, text) when is_binary(char) and byte_size(text) > byte_size(char) do
    if String.starts_with?(text, char), do: char_literal(text, byte_size(char)), else: one(text)
  end

  defp token(_table, text), do: one(text)

  # `word:`, a keyword list's key, and not `word::`.
  defp key?(text, n), do: at?(text, n, ":") and not at?(text, n, "::") and not at?(text, n + 1, ":")

  # A character literal: its start, then one character or an escape and one.
  defp char_literal(text, n) do
    rest = binary_part(text, n, byte_size(text) - n)

    case rest do
      <<?\\, c::utf8, _::binary>> -> {n + 1 + byte_size(<<c::utf8>>), :constant}
      <<c::utf8, _::binary>> -> {n + byte_size(<<c::utf8>>), :constant}
      _ -> {n, :normal}
    end
  end

  # One character, or one byte that is not UTF-8.
  defp one(<<c::utf8, _::binary>>), do: {byte_size(<<c::utf8>>), :normal}
  defp one(_text), do: {1, :normal}

  defp span(text, fun), do: span(text, fun, 0)

  defp span(text, fun, n) do
    if n < byte_size(text) and fun.(:binary.at(text, n)), do: span(text, fun, n + 1), else: n
  end

  defp word?(c), do: c in ?a..?z or c in ?A..?Z or c in ?0..?9 or c in [?_, ??, ?!]
  defp number?(c), do: c in ?a..?z or c in ?A..?Z or c in ?0..?9 or c in [?_, ?.]
end
