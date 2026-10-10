defmodule Redoubt.ContextsTest do
  use ExUnit.Case, async: true

  alias Redoubt.Contexts

  test "the steward's listing reads as maps, the default context's name empty" do
    text = "\tattached\t3\nwork\tdetached\t340\n"

    assert Contexts.parse(text) == [
             %{name: "", state: :attached, age: 3},
             %{name: "work", state: :detached, age: 340}
           ]
  end

  test "a line in any other form is left out" do
    assert Contexts.parse("a\tasleep\t1\nb\tattached\tx\nc\tdetached\nd\tattached\t2\n") ==
             [%{name: "d", state: :attached, age: 2}]
  end
end
