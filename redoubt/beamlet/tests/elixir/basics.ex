defmodule Basics do
  # Pipes, Enum, pattern matching, strings, structs and protocols.
  defmodule Point do
    defstruct x: 0, y: 0
  end

  def start do
    squares = 1..10 |> Enum.map(&(&1 * &1)) |> Enum.filter(&(rem(&1, 2) == 0))
    {first, rest} = List.pop_at([1, 2, 3], 0)
    p = %Point{x: 1}
    p2 = %{p | y: 5}
    word_lengths = "the quick brown fox" |> String.split() |> Enum.map(&String.length/1)

    {squares, Enum.sum(squares), first, rest, p2.y, Map.from_struct(p2) |> Map.to_list() |> Enum.sort(),
     word_lengths, String.upcase("hello"), "a" <> "b", Enum.reduce([1, 2, 3], 0, &+/2),
     Enum.zip([1, 2], [:a, :b]), Keyword.get([a: 1, b: 2], :b), Integer.to_string(255, 16),
     for(x <- [1, 2, 3], y <- [:a], do: {x, y}), with({:ok, v} <- {:ok, 42}, do: v * 2),
     case {1, 2} do
       {a, b} when a < b -> :lt
       _ -> :other
     end}
  end
end
