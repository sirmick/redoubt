defmodule Redoubt.Agents.NoSink do
  @moduledoc """
  A scripted hostile agent that is labelled and looks for a sink (docs/plan/m3-agents.md, "A
  scripted hostile agent"; docs/userland/agents.md, "Leases"; docs/servers/ipd.md, R60). It runs
  in a labelled lease's VM beside `Redoubt.Agents.Echo`, started by an approval at `approve@box`,
  its `/project` the label set's vault. It tries to reach the network: a connect to the bench's
  peer, to the box's own listed address and to the loopback address. Its namespace holds no
  `/net`, so beamlet's `gen_tcp` has only the loopback inside this one VM
  (docs/userland/beamlet.md, "beamlet on Redoubt"): every other address is `enetunreach` before
  any request leaves the VM, and the loopback finds no listener. The agent never reaches `ipd`,
  which would refuse a labelled caller before reading its request (R60, `net-attacks`): the
  namespace is the first wall and `ipd` the second.

  The record it writes to `/project/out/result` is read by the label set's own session, from the
  vault the agent cannot leave; the verdict that no frame left the box is the bench peer's,
  which counts no connection from it (docs/testbench.md, "Rule F"). The agent writes the record
  and returns, so its lease ends on its own and the steward's audit holds the end.
  """

  @output "/project/out/result"

  @doc "Entry: `Redoubt.Agents.Echo` dispatches the `nosink` order here."
  def run do
    record = [
      # What the lease holds: no `/net` in its table, and nothing at the network clone path.
      net: Enum.any?(Redoubt.Namespace.table(), &(elem(&1, 0) == "/net")),
      net_clone: probe(fn -> Redoubt.Namespace.lookup("/net/tcp/clone") end),
      # The bench's peer, the box's own listed address and the loopback: without a `/net` the
      # first two are unreachable to this VM, and the third finds no listener in it.
      far: probe(fn -> :gen_tcp.connect({10, 0, 9, 101}, 80, [:binary, active: false], 20_000) end),
      listed: probe(fn -> :gen_tcp.connect({10, 0, 2, 15}, 22, [:binary, active: false], 20_000) end),
      loopback: probe(fn -> :gen_tcp.connect({127, 0, 0, 1}, 22, [:binary, active: false], 20_000) end)
    ]

    File.mkdir_p("/project/out")
    # Written and read back before the agent returns: the VM's end tears its connections down,
    # and a write still on its way would be lost with them (as `Redoubt.Agents.Echo` does).
    File.write(@output, inspect(record, limit: :infinity) <> "\n")
    _ = File.read(@output)
    :ok
  end

  # Each attempt's outcome, caught so one refusal cannot stop the rest.
  defp probe(fun) do
    fun.()
  rescue
    e -> {:raised, Exception.message(e)}
  catch
    kind, reason -> {:caught, kind, reason}
  end
end
