defmodule Redoubt.Term.Buffer do
  @moduledoc """
  The screen buffer, beamlet's natives `redoubt_screen` (docs/userland/beamlet.md, "Screen
  natives"), as the shell calls them: text is split into graphemes here, by OTP's own
  segmentation, and a style is a `{fg, bg, modifiers}` tuple.

  The natives refuse a control character with `badarg`: a caller makes text visible first
  (`Redoubt.Term.Text.visible/1`), and one that forgot fails loudly instead of drawing it.

  The buffer is beamlet's: on the BEAM there is none, and `available?/0` says so.
  """

  import Bitwise

  @type t :: reference()
  @type color :: :reset | {:indexed, 0..255} | {:rgb, 0..255, 0..255, 0..255}
  @type style :: {color(), color(), non_neg_integer()}
  @type rect :: {non_neg_integer(), non_neg_integer(), non_neg_integer(), non_neg_integer()}

  @modifiers %{
    bold: 1,
    dim: 2,
    italic: 4,
    underlined: 8,
    slow_blink: 16,
    rapid_blink: 32,
    reversed: 64,
    hidden: 128,
    crossed_out: 256
  }

  # The natives are beamlet's, so they are called by name: the BEAM compiles this without a word.
  @natives :redoubt_screen

  @doc "Whether this VM has the screen buffer's natives (beamlet has; the BEAM has not)."
  @spec available?() :: boolean()
  def available? do
    # A reference that is no buffer: beamlet refuses it (badarg), the BEAM has no such function.
    _ = apply(@natives, :diff, [make_ref()])
    true
  rescue
    ArgumentError -> true
    UndefinedFunctionError -> false
  end

  @doc "A blank buffer of `cols` by `rows`, owned by the calling process."
  @spec new(pos_integer(), pos_integer()) :: t()
  def new(cols, rows), do: apply(@natives, :new, [cols, rows])

  @doc "A new size, blank: the next frame clears the screen and sends it all."
  @spec resize(t(), pos_integer(), pos_integer()) :: :ok
  def resize(buffer, cols, rows), do: apply(@natives, :resize, [buffer, cols, rows])

  @doc "Writes `text` along row `y` from column `x`, clipped; the columns written."
  @spec put(t(), non_neg_integer(), non_neg_integer(), String.t(), style()) :: non_neg_integer()
  def put(buffer, x, y, text, style \\ plain()),
    do: apply(@natives, :put, [buffer, x, y, String.graphemes(text), style])

  @doc "One symbol, a single code point of one column, over a rectangle, clipped."
  @spec fill(t(), rect(), String.t(), style()) :: :ok
  def fill(buffer, rect, symbol, style \\ plain()), do: apply(@natives, :fill, [buffer, rect, symbol, style])

  @doc "A Braille bitmap over a rectangle: one byte a cell, its eight dots in Braille's order."
  @spec plot(t(), rect(), binary(), style()) :: :ok
  def plot(buffer, rect, dots, style \\ plain()), do: apply(@natives, :plot, [buffer, rect, dots, style])

  @doc "The cells changed since the last frame, as a `cells` frame's bytes."
  @spec diff(t()) :: binary()
  def diff(buffer), do: apply(@natives, :diff, [buffer])

  @doc """
  Reads a native program's `cells` frame into the buffer from byte `from`, 0 for its header, as
  the one decoder reads it, a cell at a time (`cells::reduce`, in Rust; `Redoubt.Term.Cells.reduce/3`
  is the same decoder in Elixir). `:ok` once read whole; `{:size, w, h}` for a frame of another
  size, nothing read; `{:joined, next, x, y, symbol, style}` for a cell whose symbol joins code
  points, not put: whether it is one grapheme is OTP's segmentation to say, so the caller puts it
  and goes on from `next`; or `{:error, why}`, `why` the decoder's reason or `:cell` (a wide
  symbol in the last column). A `from` that is not where the last call stopped raises.
  """
  @spec frame(t(), binary(), non_neg_integer()) ::
          :ok
          | {:size, pos_integer(), pos_integer()}
          | {:joined, pos_integer(), non_neg_integer(), non_neg_integer(), String.t(), style()}
          | {:error, Redoubt.Term.Cells.error() | :cell}
  def frame(buffer, bytes, from \\ 0), do: apply(@natives, :frame, [buffer, bytes, from])

  @doc """
  The cells changed since the last frame, as the escape sequences that draw them, which then
  become what is shown: the bytes `Redoubt.Term.Frame.draw/1` draws for `diff/1`'s frame, drawn
  in Rust from the buffer's own cells. Any process holding the buffer may draw it; only its
  owner draws into it.
  """
  @spec draw(t()) :: binary()
  def draw(buffer), do: apply(@natives, :draw, [buffer])

  @doc "The terminal's own colours and no attributes."
  @spec plain() :: style()
  def plain, do: {:reset, :reset, 0}

  @doc """
  A style from options: `:fg` and `:bg` (a colour), and any of the attributes as `true`:
  `:bold`, `:dim`, `:italic`, `:underlined`, `:slow_blink`, `:rapid_blink`, `:reversed`,
  `:hidden`, `:crossed_out`.
  """
  @spec style(keyword()) :: style()
  def style(opts) do
    bits = for {name, bit} <- @modifiers, Keyword.get(opts, name, false), reduce: 0, do: (acc -> acc ||| bit)
    {Keyword.get(opts, :fg, :reset), Keyword.get(opts, :bg, :reset), bits}
  end
end
