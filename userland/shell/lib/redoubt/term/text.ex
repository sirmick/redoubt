defmodule Redoubt.Term.Text do
  @moduledoc """
  Text made safe to draw: every control character becomes visible characters.

  Whatever the shell draws may have been written by a hostile party (a file's contents, a file
  name, an exception's message), and a terminal obeys control sequences in what it is sent: an
  OSC 52 sets the person's clipboard, an OSC 8 forges a link, a status query makes the terminal
  answer as if typed. So nothing reaches the terminal as one. A C0 control is drawn in caret
  notation (`^[` for ESC, `^G` for BEL), DEL as `^?`, a C1 control as `<U+009B>` (some
  terminals obey U+009B as ESC [), a bidirectional embedding, override or isolate control as
  `<U+202E>` (they reorder what is shown around them), an invisible format character as
  `<U+200B>` (with a zero-width space, one `rm report.txt` would look like another), and a byte
  that is not UTF-8 as `<FF>`, as `less` does. A tab is expanded to spaces. What counts as a control
  character is `control?/1`, the `cells` crate's rule too, stated once in
  `userland/native/cells/forbidden.txt`; the one exception is the zero-width joiner inside an
  emoji ZWJ sequence (`👩\u200D💻`), one grapheme that draws as one (`joined?/2`).

  `Redoubt.Term`'s encoder draws every grapheme through this rule, and a screen program's text
  passes it before the buffer's natives, which refuse a control character (docs/userland/shell.md,
  "Hostile text never drives the terminal"). Tab stops count code points, not display width.
  """

  @tab 8

  # The bytes of text the first call of the VM's matchers on a run looks at, and the most any
  # looks at (`printable/1`).
  @first 128
  @most 512

  @joiner 0x200D

  # The control characters, as ranges: C0, DEL and C1, the soft hyphen, the zero-width space,
  # non-joiner and joiner, the line and paragraph separators, the bidirectional embeddings and
  # overrides, the word joiner and the invisible operators, the bidirectional isolates, the byte
  # order mark, and the tags of plane 14 (`forbidden.txt`).
  @controls [
    0x00..0x1F,
    0x7F..0x9F,
    0xAD..0xAD,
    0x200B..0x200D,
    0x2028..0x2029,
    0x202A..0x202E,
    0x2060..0x2064,
    0x2066..0x2069,
    0xFEFF..0xFEFF,
    0xE0000..0xE007F
  ]

  # Extended_Pictographic: Unicode 16.0's 78 ranges, as the pinned OTP has them (`forbidden.txt`).
  @pictographic [
    0xA9..0xA9,
    0xAE..0xAE,
    0x203C..0x203C,
    0x2049..0x2049,
    0x2122..0x2122,
    0x2139..0x2139,
    0x2194..0x2199,
    0x21A9..0x21AA,
    0x231A..0x231B,
    0x2328..0x2328,
    0x2388..0x2388,
    0x23CF..0x23CF,
    0x23E9..0x23F3,
    0x23F8..0x23FA,
    0x24C2..0x24C2,
    0x25AA..0x25AB,
    0x25B6..0x25B6,
    0x25C0..0x25C0,
    0x25FB..0x25FE,
    0x2600..0x2605,
    0x2607..0x2612,
    0x2614..0x2685,
    0x2690..0x2705,
    0x2708..0x2712,
    0x2714..0x2714,
    0x2716..0x2716,
    0x271D..0x271D,
    0x2721..0x2721,
    0x2728..0x2728,
    0x2733..0x2734,
    0x2744..0x2744,
    0x2747..0x2747,
    0x274C..0x274C,
    0x274E..0x274E,
    0x2753..0x2755,
    0x2757..0x2757,
    0x2763..0x2767,
    0x2795..0x2797,
    0x27A1..0x27A1,
    0x27B0..0x27B0,
    0x27BF..0x27BF,
    0x2934..0x2935,
    0x2B05..0x2B07,
    0x2B1B..0x2B1C,
    0x2B50..0x2B50,
    0x2B55..0x2B55,
    0x3030..0x3030,
    0x303D..0x303D,
    0x3297..0x3297,
    0x3299..0x3299,
    0x1F000..0x1F0FF,
    0x1F10D..0x1F10F,
    0x1F12F..0x1F12F,
    0x1F16C..0x1F171,
    0x1F17E..0x1F17F,
    0x1F18E..0x1F18E,
    0x1F191..0x1F19A,
    0x1F1AD..0x1F1E5,
    0x1F201..0x1F20F,
    0x1F21A..0x1F21A,
    0x1F22F..0x1F22F,
    0x1F232..0x1F23A,
    0x1F23C..0x1F23F,
    0x1F249..0x1F3FA,
    0x1F400..0x1F53D,
    0x1F546..0x1F64F,
    0x1F680..0x1F6FF,
    0x1F774..0x1F77F,
    0x1F7D5..0x1F7FF,
    0x1F80C..0x1F80F,
    0x1F848..0x1F84F,
    0x1F85A..0x1F85F,
    0x1F888..0x1F88F,
    0x1F8AE..0x1F8FF,
    0x1F90C..0x1F93A,
    0x1F93C..0x1F945,
    0x1F947..0x1FAFF,
    0x1FC00..0x1FFFD
  ]

  @doc """
  Whether `c` is a control character: C0, DEL, C1, a bidirectional embedding, override or
  isolate control, an invisible format character (the soft hyphen, the zero-width characters and
  joiners, the byte order mark, the tags) or a line or paragraph separator. None is ever drawn as
  itself, and none may be a cell's symbol, but for the joiner as `joined?/2` says.
  """
  @spec control?(non_neg_integer()) :: boolean()
  for range <- @controls do
    def control?(c) when c >= unquote(range.first) and c <= unquote(range.last), do: true
  end

  def control?(_c), do: false

  @doc "The control characters, as ranges of code points."
  @spec controls() :: [Range.t()]
  def controls, do: @controls

  @doc "Whether `c` is Extended_Pictographic."
  @spec pictographic?(non_neg_integer()) :: boolean()
  def pictographic?(c), do: Enum.any?(@pictographic, &(c in &1))

  @doc "The pictographs, as ranges of code points."
  @spec pictographs() :: [Range.t()]
  def pictographs, do: @pictographic

  @doc """
  Whether a zero-width joiner between `before` and `after` stands inside an emoji ZWJ sequence:
  `before` ends with an element, a pictograph alone or followed by the emoji presentation
  selector (U+FE0F) or a skin tone modifier (U+1F3FB to U+1F3FF), and `after` starts with a
  pictograph. There it is the grapheme's and is kept; anywhere else it is drawn as `<U+200D>`.
  """
  @spec joined?(binary(), binary()) :: boolean()
  def joined?(before, <<c::utf8, _::binary>>), do: pictographic?(c) and element?(before)
  def joined?(_before, _after), do: false

  defp element?(text) do
    case last(text) do
      {rest, c} when c == 0xFE0F or c in 0x1F3FB..0x1F3FF -> pictograph_last?(rest)
      {_rest, c} -> pictographic?(c)
      nil -> false
    end
  end

  defp pictograph_last?(text) do
    case last(text) do
      {_rest, c} -> pictographic?(c)
      nil -> false
    end
  end

  # The code point `text` ends with, and the text before it; nil for none.
  defp last(<<>>), do: nil

  defp last(text) do
    at = start(text, byte_size(text) - 1)

    case binary_part(text, at, byte_size(text) - at) do
      <<c::utf8>> -> {binary_part(text, 0, at), c}
      _not_utf8 -> nil
    end
  end

  defp start(text, at) when at > 0 do
    case binary_part(text, at, 1) do
      <<b>> when b in 0x80..0xBF -> start(text, at - 1)
      _lead -> at
    end
  end

  defp start(_text, at), do: at

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
    {acc, col} = scan(text, text, col, [])
    {IO.iodata_to_binary(acc), col}
  end

  # `text` is what is left of `all`, the text being made visible: a joiner is judged by what stands
  # before it in `all` (a grapheme cut by a piece's edge is drawn as parts, as it is measured).
  defp scan(<<>>, _all, col, acc), do: {Enum.reverse(acc), col}

  defp scan(<<?\t, rest::binary>>, all, col, acc) do
    n = @tab - rem(col, @tab)
    scan(rest, all, col + n, [:binary.copy(" ", n) | acc])
  end

  # A run of C0 controls and DELs is drawn in caret notation into one binary, which grows in place:
  # text of many of them costs a step a byte and leaves no garbage a byte (`carets/3`).
  defp scan(<<c, _::binary>> = text, all, col, acc) when c < 0x20 or c == 0x7F do
    {drawn, col, rest} = carets(text, col, <<>>)
    scan(rest, all, col, [drawn | acc])
  end

  # A joiner inside an emoji sequence is kept, as `Redoubt.Term.Cells` keeps it in a symbol.
  defp scan(<<@joiner::utf8, rest::binary>> = text, all, col, acc) do
    before = binary_part(all, 0, byte_size(all) - byte_size(text))

    if joined?(before, rest),
      do: scan(rest, all, col + 1, [<<@joiner::utf8>> | acc]),
      else: unprintable(text, all, col, acc)
  end

  # A run of printable text, UTF-8 with no control character, is kept as it is, one slice of the
  # text, its code points counted for the tab stops; anything else is taken alone.
  defp scan(text, all, col, acc) do
    case printable(text) do
      {0, _} ->
        unprintable(text, all, col, acc)

      {bytes, points} ->
        <<run::binary-size(^bytes), rest::binary>> = text
        scan(rest, all, col + points, [run | acc])
    end
  end

  defp carets(<<c, rest::binary>>, col, drawn) when c < 0x20 and c != ?\t,
    do: carets(rest, col + 2, <<drawn::binary, ?^, c + 64>>)

  defp carets(<<0x7F, rest::binary>>, col, drawn), do: carets(rest, col + 2, <<drawn::binary, "^?">>)
  defp carets(rest, col, drawn), do: {drawn, col, rest}

  # What `printable/1` stops at before any byte: a control character above ASCII (the C0 controls
  # and DEL never reach here), drawn as `<U+200B>`, or a byte that is not UTF-8.
  defp unprintable(<<c::utf8, rest::binary>>, all, col, acc) when c >= 0x80 do
    drawn = ["<U+", point(c), ">"]
    scan(rest, all, col + IO.iodata_length(drawn), [drawn | acc])
  end

  defp unprintable(<<b, rest::binary>>, all, col, acc), do: scan(rest, all, col + 4, [byte(b) | acc])

  # The bytes and the code points of the printable text `text` starts with: UTF-8 code points
  # that are not `control?/1`, by the VM's own matchers from the first byte, a window at a time
  # (`matched/4`). On the machine a step of an Erlang loop over the code points costs about what
  # a matcher spends on two bytes, and a call of one as much as a few steps, so a run of any
  # length is quickest in the matchers' few calls: a line of 80 is one.
  defp printable(text), do: matched(text, @first, 0, 0)

  # The run on from `text`'s start, by the VM's matchers: a window of `size` bytes, and while the
  # run fills it the next, four times as long, up to `@most`. In a window, the run ends at the
  # earlier of its first control character, found by `stop/2`, and its first byte that is not
  # UTF-8, found by `:unicode`; a control character's encoding starts a code point wherever it
  # matches in UTF-8 (its lead byte is never another's continuation).
  defp matched(text, size, bytes, points) do
    window = binary_part(text, 0, min(byte_size(text), size))
    head = binary_part(window, 0, stop(window, 0))

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

  # Where the first control character in `window` at or after `from` starts, or the window's
  # end: the matcher finds the next byte that may start one (`controls_pattern/0`), and a
  # character that starts with such bytes but is no control (a no-break sign, a dash) is stepped
  # over, as is one the window cuts, which the UTF-8 check ends the run before.
  defp stop(window, from) do
    case :binary.match(window, controls_pattern(), scope: {from, byte_size(window) - from}) do
      :nomatch ->
        byte_size(window)

      {at, _length} ->
        case binary_part(window, at, min(4, byte_size(window) - at)) do
          <<c::utf8, _::binary>> ->
            if control?(c), do: at, else: stop(window, at + byte_size(<<c::utf8>>))

          _cut_or_not_utf8 ->
            at
        end
    end
  end

  # The bytes a control character's UTF-8 may start with, compiled for `:binary.match/3` once per
  # VM: the C0 controls and DEL as themselves, and for the rest the shortest prefix no other
  # control shares with a character outside the table's blocks: `C2` (C1, the soft hyphen),
  # `E2 80` and `E2 81` (the zero-width characters, the separators, the bidirectional controls,
  # the joiners), `EF BB` (the byte order mark) and `F3 A0` (the tags). Few patterns, since
  # beamlet's matcher tries each at every byte.
  defp controls_pattern do
    case :persistent_term.get({__MODULE__, :controls}, nil) do
      nil ->
        bytes = Enum.map(Enum.concat([0..0x1F, [0x7F]]), &<<&1>>)
        prefixes = [<<0xC2>>, <<0xE2, 0x80>>, <<0xE2, 0x81>>, <<0xEF, 0xBB>>, <<0xF3, 0xA0>>]
        controls = :binary.compile_pattern(bytes ++ prefixes)
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
  defp byte(b), do: ["<", hex(b, 2), ">"]

  # A code point, in hex of at least four digits, as `U+` names it.
  defp point(c), do: hex(c, 4)

  defp hex(n, digits), do: n |> Integer.to_string(16) |> String.pad_leading(digits, "0")
end
