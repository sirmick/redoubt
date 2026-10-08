defmodule Redoubt.Screen.Layout do
  @moduledoc """
  Layout is rectangles only (docs/userland/shell.md, "Full-screen programs"): a rectangle is
  `{x, y, w, h}` in cells, and a screen splits, centres and insets its own.
  """

  @type rect :: {non_neg_integer(), non_neg_integer(), non_neg_integer(), non_neg_integer()}
  @type constraint :: {:fixed, non_neg_integer()} | {:percent, 0..100} | :rest

  @doc """
  Splits `rect` into rows (`:rows`, stacked top to bottom) or columns (`:cols`, left to right),
  one for each constraint: `{:fixed, n}` cells, `{:percent, p}` of the whole, and `:rest`, an
  equal share of what the others leave, the first ones taking a cell more when it does not
  divide. What does not fit is cut from the last; nothing is ever negative.
  """
  @spec split(rect(), :rows | :cols, [constraint()]) :: [rect()]
  def split({x, y, w, h}, direction, constraints) do
    total = if direction == :rows, do: h, else: w

    fixed =
      Enum.map(constraints, fn
        {:fixed, n} -> n
        {:percent, p} -> div(total * p, 100)
        :rest -> 0
      end)

    rests = Enum.count(constraints, &(&1 == :rest))
    left = max(total - Enum.sum(fixed), 0)

    # Each in turn, with the count of rests before it and the cells used before it.
    constraints
    |> Enum.zip(fixed)
    |> Enum.map_reduce({0, 0}, fn {c, n}, {i, used} ->
      {size, i} =
        if c == :rest,
          do: {div(left, rests) + if(i < rem(left, rests), do: 1, else: 0), i + 1},
          else: {n, i}

      size = min(size, total - used)
      rect = if direction == :rows, do: {x, y + used, w, size}, else: {x + used, y, size, h}
      {rect, {i, used + size}}
    end)
    |> elem(0)
  end

  @doc "A rectangle of `w` by `h` centred in `rect`, no larger than it."
  @spec centre(rect(), non_neg_integer(), non_neg_integer()) :: rect()
  def centre({x, y, rw, rh}, w, h) do
    {w, h} = {min(w, rw), min(h, rh)}
    {x + div(rw - w, 2), y + div(rh - h, 2), w, h}
  end

  @doc "`rect` with `n` cells taken from each side, never less than empty."
  @spec inset(rect(), non_neg_integer()) :: rect()
  def inset({x, y, w, h}, n) do
    {w2, h2} = {max(w - 2 * n, 0), max(h - 2 * n, 0)}
    {x + min(n, div(w, 2)), y + min(n, div(h, 2)), w2, h2}
  end
end
