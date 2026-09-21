defmodule Procs do
  # spawn/send/receive, Task-like patterns, and an Agent-free GenServer-free echo.
  def start do
    parent = self()
    pid = spawn(fn -> receive do {:ping, from} -> send(from, :pong) end end)
    send(pid, {:ping, parent})
    r = receive do
      :pong -> :got_pong
    after 1000 -> :timeout
    end

    results =
      for n <- 1..5 do
        spawn(fn -> send(parent, {:sq, n * n}) end)
      end
      |> Enum.map(fn _ -> receive do {:sq, v} -> v end end)
      |> Enum.sort()

    {r, results}
  end
end
