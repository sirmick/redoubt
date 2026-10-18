defmodule Redoubt.Agents.Churn do
  @moduledoc """
  A scripted agent that writes `/project` without end (docs/plan/m3-agents.md, "A scripted hostile
  agent"; docs/kernel/ipc.md, R3; docs/servers/steward.md, "Leases"). It runs in a lease's VM
  beside `Redoubt.Agents.Echo`, dispatched by the `writeloop` order. It writes a marker its sponsor
  can wait on, then writes a page-sized buffer to `/project/out/churn` in a tight loop, so a `Twrite`
  lend to `walfsd` is almost always in flight. When the sponsor ends the lease mid-write, the kernel
  destroys the agent's budget while `walfsd` holds its lent pages: R3 (lends and abandoned calls)
  says `walfsd` frees them and serves on. The verdict is `walfsd`, which does not fault, and the
  sponsor's own reads afterward (docs/testbench.md, "Rule F"), never anything this agent wrote.
  """

  @started "/project/out/started"
  @churn "/project/out/churn"

  @doc "Entry: `Redoubt.Agents.Echo` dispatches the `writeloop` order here."
  def run do
    File.mkdir_p("/project/out")
    File.write(@started, "churning\n")
    loop(:binary.copy("x", 4096))
  end

  # Each turn is a whole write of the buffer to `walfsd` through `/project`: a lend that the lease's
  # end abandons in flight.
  defp loop(data) do
    File.write(@churn, data)
    loop(data)
  end
end
