# IEx's own read-eval-print loop, over the console (input in Elixir.IexTest.stdin). The
# result is how the session ended; its transcript is the console output before it.
defmodule IexTest do
  def start do
    {:ok, _} = Application.ensure_all_started(:iex)
    IEx.Server.run([])
  end
end
