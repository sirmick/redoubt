defmodule Redoubt.Term.Width do
  @moduledoc """
  How many columns text takes on a terminal: two for a wide character (most of CJK, Hangul, the
  fullwidth forms and the emoji), one for anything else.

  This is an approximation by ranges of code points, enough to lay out a table; the encoder's own
  width tables, from Unicode's East Asian Width data, replace it with the terminal driver
  (docs/userland/shell.md, "The terminal library").
  """

  @wide [
    0x1100..0x115F,
    0x2E80..0x303E,
    0x3041..0x33FF,
    0x3400..0x4DBF,
    0x4E00..0x9FFF,
    0xA000..0xA4CF,
    0xAC00..0xD7A3,
    0xF900..0xFAFF,
    0xFE30..0xFE4F,
    0xFF00..0xFF60,
    0xFFE0..0xFFE6,
    0x1F300..0x1F64F,
    0x1F900..0x1F9FF,
    0x20000..0x2FFFD,
    0x30000..0x3FFFD
  ]

  @doc "The columns `text` takes: each grapheme two if it starts with a wide character, else one."
  @spec columns(String.t()) :: non_neg_integer()
  def columns(text), do: text |> String.graphemes() |> Enum.map(&grapheme/1) |> Enum.sum()

  defp grapheme(<<c::utf8, _rest::binary>>), do: if(Enum.any?(@wide, &(c in &1)), do: 2, else: 1)
  defp grapheme(_not_utf8), do: 1
end
