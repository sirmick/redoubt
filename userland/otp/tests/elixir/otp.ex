defmodule Counter do
  use GenServer
  def init(n), do: {:ok, n}
  def handle_call(:get, _from, n), do: {:reply, n, n}
  def handle_call({:add, k}, _from, n), do: {:reply, n + k, n + k}
  def handle_cast(:reset, _n), do: {:noreply, 0}
end

defmodule OtpTest do
  # GenServer, Agent, Task and Supervisor from the real Elixir standard library.
  def start do
    {:ok, pid} = GenServer.start_link(Counter, 10)
    a = GenServer.call(pid, :get)
    b = GenServer.call(pid, {:add, 5})
    GenServer.cast(pid, :reset)
    c = GenServer.call(pid, :get)

    {:ok, agent} = Agent.start_link(fn -> %{} end)
    Agent.update(agent, &Map.put(&1, :k, 1))
    d = Agent.get(agent, & &1)

    e = Task.async(fn -> 6 * 7 end) |> Task.await()
    f = Enum.map([1, 2, 3], &Task.async(fn -> &1 * 2 end)) |> Enum.map(&Task.await/1)

    children = [%{id: Counter, start: {GenServer, :start_link, [Counter, 100, [name: :named_counter]]}}]
    {:ok, sup} = Supervisor.start_link(children, strategy: :one_for_one)
    g = GenServer.call(:named_counter, :get)
    [{Counter, child, :worker, _}] = Supervisor.which_children(sup)
    Process.exit(child, :kill)
    h = wait_restart(child)
    i = GenServer.call(:named_counter, :get)

    {a, b, c, d, e, f, g, h, i}
  end

  defp wait_restart(old) do
    case Process.whereis(:named_counter) do
      pid when is_pid(pid) and pid != old -> :restarted
      _ -> Process.sleep(1); wait_restart(old)
    end
  end
end
