defmodule Redoubt.Screen.LayoutTest do
  use ExUnit.Case, async: true

  alias Redoubt.Screen.Layout

  test "a split by fixed size, percentage and what is left" do
    assert Layout.split({0, 0, 80, 24}, :rows, [{:fixed, 1}, :rest, {:fixed, 1}]) ==
             [{0, 0, 80, 1}, {0, 1, 80, 22}, {0, 23, 80, 1}]

    assert Layout.split({2, 3, 100, 10}, :cols, [{:percent, 30}, :rest]) == [{2, 3, 30, 10}, {32, 3, 70, 10}]
  end

  test "what is left is shared, the first shares taking a cell more when it does not divide" do
    assert Layout.split({0, 0, 10, 1}, :cols, [:rest, :rest, :rest]) == [
             {0, 0, 4, 1},
             {4, 0, 3, 1},
             {7, 0, 3, 1}
           ]
  end

  test "what does not fit is cut from the last, and nothing is negative" do
    assert Layout.split({0, 0, 5, 5}, :rows, [{:fixed, 3}, {:fixed, 4}, :rest]) ==
             [{0, 0, 5, 3}, {0, 3, 5, 2}, {0, 5, 5, 0}]
  end

  test "centred, and never larger than what it is centred in" do
    assert Layout.centre({0, 0, 80, 24}, 20, 10) == {30, 7, 20, 10}
    assert Layout.centre({5, 5, 10, 4}, 40, 40) == {5, 5, 10, 4}
  end

  test "inset, and never less than empty" do
    assert Layout.inset({0, 0, 10, 6}, 1) == {1, 1, 8, 4}
    assert Layout.inset({0, 0, 3, 1}, 2) == {1, 0, 0, 0}
  end
end
