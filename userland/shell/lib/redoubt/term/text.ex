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

  # A run's first code points taken one at a time, then the bytes of text the first call of the
  # VM's matchers on it looks at, and the most any looks at (`printable/1`).
  @steps 64
  @first 128
  @most 512

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

  defp scan(<<?\t, rest::binary>>, col, acc) do
    n = @tab - rem(col, @tab)
    scan(rest, col + n, [:binary.copy(" ", n) | acc])
  end

  # A run of C0 controls and DELs is drawn in caret notation into one binary, which grows in place:
  # text of many of them costs a step a byte and leaves no garbage a byte (`carets/3`).
  defp scan(<<c, _::binary>> = text, col, acc) when c < 0x20 or c == 0x7F do
    {drawn, col, rest} = carets(text, col, <<>>)
    scan(rest, col, [drawn | acc])
  end

  # A run of printable text, UTF-8 with no control character, is kept as it is, one slice of the
  # text, its code points counted for the tab stops; anything else is taken alone.
  defp scan(text, col, acc) do
    case printable(text) do
      {0, _} ->
        unprintable(text, col, acc)

      {bytes, points} ->
        <<run::binary-size(^bytes), rest::binary>> = text
        scan(rest, col + points, [run | acc])
    end
  end

  defp carets(<<c, rest::binary>>, col, drawn) when c < 0x20 and c != ?\t,
    do: carets(rest, col + 2, <<drawn::binary, ?^, c + 64>>)

  defp carets(<<0x7F, rest::binary>>, col, drawn), do: carets(rest, col + 2, <<drawn::binary, "^?">>)
  defp carets(rest, col, drawn), do: {drawn, col, rest}

  defp unprintable(<<c::utf8, rest::binary>>, col, acc) when c in 0x80..0x9F,
    do: scan(rest, col + 8, [["<U+00", hex(c), ">"] | acc])

  defp unprintable(<<c::utf8, rest::binary>>, col, acc) when c in 0x202A..0x202E or c in 0x2066..0x2069,
    do: scan(rest, col + 8, [["<U+", hex(c), ">"] | acc])

  defp unprintable(<<b, rest::binary>>, col, acc), do: scan(rest, col + 4, [byte(b) | acc])

  # The bytes and the code points of the printable text `text` starts with: UTF-8 code points
  # that are not `control?/1`. Its first `@steps` are taken one at a time in Erlang; a run longer
  # than that goes on by the VM's own matchers, a window at a time (`matched/4`): on the machine
  # an Erlang step costs as much as a matcher spends on hundreds of bytes, while a call of one
  # costs as much as several steps, so text of short runs is quickest a code point at a time and a
  # long run in few calls.
  defp printable(text), do: printable(text, 0, 0, @steps)

  defp printable(<<c, rest::binary>>, bytes, points, left) when left > 0 and c in 0x20..0x7E,
    do: printable(rest, bytes + 1, points + 1, left - 1)

  defp printable(<<c::utf8, rest::binary>>, bytes, points, left)
       when left > 0 and c >= 0xA0 and c not in 0x202A..0x202E and c not in 0x2066..0x2069,
       do: printable(rest, bytes + byte_size(<<c::utf8>>), points + 1, left - 1)

  defp printable(text, bytes, points, 0), do: matched(text, @first, bytes, points)
  defp printable(_text, bytes, points, _left), do: {bytes, points}

  # The run on from `text`'s start, by the VM's matchers: a window of `size` bytes, and while the
  # run fills it the next, four times as long, up to `@most`. In a window, the run ends at the
  # earlier of its first control character, found by `:binary.match/2`, and its first byte that is
  # not UTF-8, found by `:unicode`; a control character's encoding starts a code point wherever it
  # matches in UTF-8 (its lead byte is never another's continuation).
  defp matched(text, size, bytes, points) do
    window = binary_part(text, 0, min(byte_size(text), size))

    head =
      case :binary.match(window, controls()) do
        {at, _length} -> binary_part(window, 0, at)
        :nomatch -> window
      end

    # The code points, and where the UTF-8 ends, from one call that copies no bytes: a binary made
    # to check them would count toward the driver's heap limit until collected.
    {n, points} =
      case :unicode.characters_to_list(head) do
        chars when is_list(chars) -> {byte_size(head), points + length(chars)}
        {_error, chars, rest} -> {byte_size(head) - byte_size(rest), points + length(chars)}
      end

    if n == byte_size(window) and n < byte_size(text) do
      <<_::binary-size(^n), rest::binary>> = text
      matched(rest, min(size * 4, @most), bytes + n, points)
    else
      {bytes + n, points}
    end
  end

  # Every control character's UTF-8, compiled for `:binary.match/2` once per VM.
  defp controls do
    case :persistent_term.get({__MODULE__, :controls}, nil) do
      nil ->
        controls =
          [0..0x1F, 0x7F..0x9F, 0x202A..0x202E, 0x2066..0x2069]
          |> Enum.concat()
          |> Enum.map(&<<&1::utf8>>)
          |> :binary.compile_pattern()

        :persistent_term.put({__MODULE__, :controls}, controls)
        controls

      controls ->
        controls
    end
  end

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
