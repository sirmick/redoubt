defmodule Redoubt.Screen.Widget.Canvas do
  @moduledoc """
  A Braille canvas: a grid of dots, two across and four down in each cell, drawn with the screen
  buffer's `plot` (`Redoubt.Term.Buffer.plot/4`) as Braille patterns. It holds dots, not text, so
  it has no keys and nothing in it can be a control character.

  A dot is `{x, y}` from the top left, in dots; one outside the canvas is not drawn.
  """

  import Bitwise

  alias Redoubt.Term.Buffer

  # The cells' bytes, by cell index, for the cells with a dot.
  defstruct cols: 0, rows: 0, cells: %{}

  @type t :: %__MODULE__{}

  # A dot's bit in its cell's byte, by its place in the cell: Braille's own numbering, dots 1-3
  # down the left, 4-6 down the right, then 7 and 8 along the bottom.
  @bits {{0, 3}, {1, 4}, {2, 5}, {6, 7}}

  @doc "A blank canvas of `cols` by `rows` cells: `2 * cols` by `4 * rows` dots."
  @spec new(non_neg_integer(), non_neg_integer()) :: t()
  def new(cols, rows), do: %__MODULE__{cols: cols, rows: rows}

  @doc "The canvas's size in dots, `{width, height}`."
  @spec size(t()) :: {non_neg_integer(), non_neg_integer()}
  def size(canvas), do: {2 * canvas.cols, 4 * canvas.rows}

  @doc "The canvas with the dot at `{x, y}` set."
  @spec set(t(), {integer(), integer()}) :: t()
  def set(%{cols: cols, rows: rows} = canvas, {x, y})
      when x >= 0 and y >= 0 and x < 2 * cols and y < 4 * rows do
    cell = div(y, 4) * cols + div(x, 2)
    bit = @bits |> elem(rem(y, 4)) |> elem(rem(x, 2))
    %{canvas | cells: Map.update(canvas.cells, cell, 1 <<< bit, &(&1 ||| 1 <<< bit))}
  end

  def set(canvas, _dot), do: canvas

  @doc "The canvas with every dot in `dots` set."
  @spec points(t(), Enumerable.t()) :: t()
  def points(canvas, dots), do: Enum.reduce(dots, canvas, &set(&2, &1))

  @doc "The canvas with a straight line of dots from `from` to `to`, both ends included."
  @spec line(t(), {integer(), integer()}, {integer(), integer()}) :: t()
  def line(canvas, {x0, y0}, {x1, y1}) do
    steps = max(abs(x1 - x0), abs(y1 - y0))

    if steps == 0 do
      set(canvas, {x0, y0})
    else
      points(
        canvas,
        for(i <- 0..steps, do: {x0 + round((x1 - x0) * i / steps), y0 + round((y1 - y0) * i / steps)})
      )
    end
  end

  @doc "Draws the canvas at `rect`'s top left, cut to `rect`, in `style`."
  @spec draw(t(), Buffer.t(), Buffer.rect(), Buffer.style()) :: :ok
  def draw(canvas, buffer, {x, y, w, h}, style \\ Buffer.plain()) do
    {w, h} = {min(w, canvas.cols), min(h, canvas.rows)}

    if w > 0 and h > 0 do
      dots =
        for r <- 0..(h - 1),
            c <- 0..(w - 1),
            into: <<>>,
            do: <<Map.get(canvas.cells, r * canvas.cols + c, 0)>>

      Buffer.plot(buffer, {x, y, w, h}, dots, style)
    end

    :ok
  end
end
