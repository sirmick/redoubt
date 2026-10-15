defmodule Redoubt.Term.Width do
  @moduledoc """
  How many columns text takes on a terminal: two for a wide grapheme, one for any other.

  Wide is OTP's own judgement, `:unicode_util.is_wide/1`, at the pinned OTP's Unicode version:
  OTP's line editor measures with it, and the screen buffer's natives with a table generated from
  it (`userland/otp/tools/gen-width.escript`), so the shell, `edlin` and a screen lay text out
  alike (docs/userland/beamlet.md, "Screen natives"). A grapheme is wide if a presentation
  selector follows its first code point or any of its code points is wide. A byte that is not
  UTF-8 takes one column, as the guard draws it.
  """

  @doc "The columns `text` takes, grapheme by grapheme."
  @spec columns(String.t()) :: non_neg_integer()
  def columns(text), do: text |> String.graphemes() |> Enum.map(&grapheme/1) |> Enum.sum()

  # The bytes of text the first call of the VM's matcher on a run looks at, and the most any looks
  # at (`run/1`).
  @first 128
  @most 512

  @doc """
  The bytes and the code points of the run `text` starts with of code points each a grapheme of
  its own one column wide, whatever stands beside it: printable ASCII, and the Latin, IPA, Greek
  and Cyrillic letters and signs below U+0530 that no mark is part of (not the soft hyphen, not
  Cyrillic's combining marks). The run takes a column a code point, but for its last when more
  follows, which a combining mark after it would join.
  """
  @spec run(String.t()) :: {non_neg_integer(), non_neg_integer()}
  # By the VM's matcher from the first byte, a window at a time (`matched/4`), as
  # `Redoubt.Term.Text`'s scan does, for the same reason.
  def run(text), do: matched(text, @first, 0, 0)

  # The run on from `text`'s start: a window of `size` bytes, and while the run fills it the next,
  # four times as long, up to `@most`.
  defp matched(text, size, bytes, points) do
    window = window(text, size)
    {:match, [{0, n}]} = :re.run(window, run_pattern(), capture: :first)
    points = points + length(:unicode.characters_to_list(binary_part(window, 0, n)))

    if n == byte_size(window) and n < byte_size(text) do
      <<_::binary-size(^n), rest::binary>> = text
      matched(rest, min(size * 4, @most), bytes + n, points)
    else
      {bytes + n, points}
    end
  end

  # The run's code points as one character class, compiled once per VM. `text` is UTF-8, as text
  # made visible is.
  defp run_pattern do
    case :persistent_term.get({__MODULE__, :run}, nil) do
      nil ->
        class = "^[\\x{20}-\\x{7E}\\x{A0}-\\x{AC}\\x{AE}-\\x{2FF}\\x{370}-\\x{482}\\x{48A}-\\x{52F}]*"
        {:ok, pattern} = :re.compile(class, [:unicode])
        :persistent_term.put({__MODULE__, :run}, pattern)
        pattern

      pattern ->
        pattern
    end
  end

  # The text's first `size` bytes at most, cut where a character starts.
  defp window(text, size) when byte_size(text) <= size, do: text
  defp window(text, size), do: binary_part(text, 0, char_start(text, size, size))

  defp char_start(text, at, size) do
    if at > size - 3 and :binary.at(text, at) in 0x80..0xBF, do: char_start(text, at - 1, size), else: at
  end

  @doc "The columns one grapheme takes: 2 or 1."
  @spec grapheme(String.t()) :: 1 | 2
  # One code point before U+1100, the first OTP judges wide, takes one column without asking it.
  def grapheme(<<c::utf8>>) when c < 0x1100, do: 1

  def grapheme(g) do
    case :unicode.characters_to_list(g) do
      chars when is_list(chars) and chars != [] -> if :unicode_util.is_wide(chars), do: 2, else: 1
      _not_utf8 -> 1
    end
  end
end
