defmodule Redoubt.PipelineTest do
  use ExUnit.Case, async: true

  alias Redoubt.Pipeline

  test "a pipeline's words split at each | into stages, each a name and its arguments" do
    assert Pipeline.stages(~w(grep -n x | wc -l)) == {:ok, [{"grep", ["-n", "x"]}, {"wc", ["-l"]}]}
    assert Pipeline.stages(~w(sort)) == {:ok, [{"sort", []}]}

    for words <- [[], ~w(|), ~w(a |), ~w(| a), ~w(a | | b)] do
      assert Pipeline.stages(words) == {:error, :empty_stage}, inspect(words)
    end
  end

  test "a pipeline is refused before anything starts: no stage, too many, or a program it cannot read" do
    assert Pipeline.run([]) == {:error, :empty_stage}
    stages = List.duplicate({"sort", []}, Pipeline.max_stages() + 1)
    assert Pipeline.run(stages) == {:error, :too_many}
    # Read from /boot, which a host has not: refused by name, and nothing was carved or launched.
    assert Pipeline.run([{"no-such-program", []}]) == {:error, :not_found}
  end
end
