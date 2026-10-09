defmodule Redoubt.Term.Text do
  @moduledoc """
  Text made safe to draw: every control character becomes visible characters.

  Whatever the shell draws may have been written by a hostile party (a file's contents, a file
  name, an exception's message), and a terminal obeys control sequences in what it is sent: an
  OSC 52 sets the person's clipboard, an OSC 8 forges a link, a status query makes the terminal
  answer as if typed. So nothing reaches the terminal as one. A C0 control is drawn in caret
  notation (`^[` for ESC, `^G` for BEL), DEL as `^?`, a C1 control as `<U+009B>` (some
  terminals obey U+009B as ESC [), a bidirectional embedding, override or isolate control as
  `<U+202E>` (they reorder what is shown around them), and a byte that is not UTF-8 as `<FF>`, as
  `less` does. A tab is expanded to spaces. What counts as a control character is `control?/1`,
  the `cells` crate's rule too.

  `Redoubt.Term`'s encoder draws every grapheme through this rule, and a screen program's text
  passes it before the buffer's natives, which refuse a control character (docs/userland/shell.md,
  "Hostile text never drives the terminal"). Tab stops count code points, not display width.
  """

  @tab 8

  @doc """
  Whether `c` is a control character: C0, DEL, C1, or a bidirectional embedding, override or
  isolate control. None is ever drawn as itself, and none may be a cell's symbol.
  """
  @spec control?(non_neg_integer()) :: boolean()
  def control?(c), do: c < 0x20 or c in 0x7F..0x9F or c in 0x202A..0x202E or c in 0x2066..0x2069

  @doc """
  Returns `text` with every control character, and every byte that is not UTF-8, replaced by
  visible characters. A newline is a control character too: split text into lines first.
  """
  @spec visible(binary()) :: binary()
  def visible(text) when is_binary(text), do: text |> visible(0) |> elem(0)

  @doc """
  As `visible/1`, for text that goes on a line already `col` code points in, so its tabs stop
  where the whole line's would: the visible text, and the column it ends at. Drawing a long line
  a piece at a time keeps the work of each piece to the piece.
  """
  @spec visible(binary(), non_neg_integer()) :: {binary(), non_neg_integer()}
  def visible(text, col) when is_binary(text) do
    {acc, col} = scan(text, col, [])
    {IO.iodata_to_binary(acc), col}
  end

  defp scan(<<>>, col, acc), do: {Enum.reverse(acc), col}

  defp scan(<<c, rest::binary>>, col, acc) when c in 0x20..0x7E,
    do: scan(rest, col + 1, [c | acc])

  defp scan(<<?\t, rest::binary>>, col, acc) do
    n = @tab - rem(col, @tab)
    scan(rest, col + n, [:binary.copy(" ", n) | acc])
  end

  defp scan(<<c, rest::binary>>, col, acc) when c < 0x20, do: scan(rest, col + 2, [[?^, c + 64] | acc])
  defp scan(<<0x7F, rest::binary>>, col, acc), do: scan(rest, col + 2, ["^?" | acc])

  defp scan(<<c::utf8, rest::binary>>, col, acc) when c in 0x80..0x9F,
    do: scan(rest, col + 8, [["<U+00", hex(c), ">"] | acc])

  defp scan(<<c::utf8, rest::binary>>, col, acc) when c in 0x202A..0x202E or c in 0x2066..0x2069,
    do: scan(rest, col + 8, [["<U+", hex(c), ">"] | acc])

  defp scan(<<c::utf8, rest::binary>>, col, acc), do: scan(rest, col + 1, [<<c::utf8>> | acc])
  defp scan(<<b, rest::binary>>, col, acc), do: scan(rest, col + 4, [byte(b) | acc])

  @doc """
  Returns `chardata` as UTF-8, each byte in it that is not UTF-8 written as `visible/1` draws
  one (`<FF>`); control characters stay as they are. For text that must be UTF-8 on its way to
  the encoder, as `io` requires, and is made visible there.
  """
  @spec utf8(IO.chardata()) :: binary()
  def utf8(chardata), do: chardata |> lossy() |> IO.iodata_to_binary()

  defp lossy(list) when is_list(list), do: Enum.map(list, &lossy/1)
  defp lossy(c) when is_integer(c), do: <<c::utf8>>

  defp lossy(bytes) when is_binary(bytes) do
    case :unicode.characters_to_binary(bytes) do
      text when is_binary(text) -> text
      {_error, good, <<b, rest::binary>>} -> [good, byte(b) | lossy(rest)]
    end
  end

  # A byte that is not UTF-8, in hex.
  defp byte(b), do: ["<", hex(b), ">"]

  defp hex(n), do: n |> Integer.to_string(16) |> String.pad_leading(2, "0")
end
