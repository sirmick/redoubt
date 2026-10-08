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

  @doc "The columns one grapheme takes: 2 or 1."
  @spec grapheme(String.t()) :: 1 | 2
  def grapheme(g) do
    case :unicode.characters_to_list(g) do
      chars when is_list(chars) and chars != [] -> if :unicode_util.is_wide(chars), do: 2, else: 1
      _not_utf8 -> 1
    end
  end
end
