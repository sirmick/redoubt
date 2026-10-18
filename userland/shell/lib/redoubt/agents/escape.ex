defmodule Redoubt.Agents.Escape do
  @moduledoc """
  A scripted hostile agent, trying to get out of its lease (docs/plan/m3-agents.md, "A scripted
  hostile agent"; docs/userland/agents.md, "Leases" and "Delegation only narrows";
  docs/servers/steward.md, "Leases"). It runs in a lease's VM beside `Redoubt.Agents.Echo`, in a
  namespace holding only the sponsor's `~/project` at `/project` and the system volume: no `/home`,
  no `/net`, no console, and a fresh steward connection that is a lease's, not its sponsor's. It
  tries, one after the other, to read, write and list outside `/project`, to reach the network and
  the console, to use the lease operations its badge does not hold, and to bind and look up paths
  it was never given; each is refused in place.

  The record it writes to `/project/out/result` is for diagnosis only: a verdict comes from a
  party the agent cannot impersonate (docs/testbench.md, "Rule F"), never from this file. The
  bench reads it as progress, and judges the escape by the sponsor's canaries, unchanged and read
  back by her own session, and by the steward's audit, which carries no grant for the agent beyond
  its own lease. The agent writes the record and then idles until its lease ends.
  """

  @output "/project/out/result"

  @doc "Entry: `Redoubt.Agents.Echo` dispatches the `escape` order here."
  def run do
    record = [
      # Reads, writes and a listing outside `/project`: the paths are not in the lease's
      # namespace, so each is refused before any volume is reached (R25: the label check never
      # opens another domain's file to this one).
      read_notes: probe(fn -> File.read("/home/alice/notes.txt") end),
      write_notes: probe(fn -> File.write("/home/alice/notes.txt", "pwned") end),
      list_home: probe(fn -> File.ls("/home/alice") end),
      list_bob: probe(fn -> File.ls("/home/bob") end),
      # A write to the read-only system volume, the console, and the network clone: no `/dev`, no
      # `/net`, and `/boot` takes no write.
      write_boot: probe(fn -> File.write("/boot/x", "pwned") end),
      write_console: probe(fn -> File.write("/dev/cons", "pwned") end),
      net_clone: probe(fn -> Redoubt.Namespace.lookup("/net/tcp/clone") end),
      # The lease operations: a lease's badge is told `unknown` (R41), so the agent cannot start a
      # sibling or use its sponsor's side of the steward, even with a term inside `MAX_LEASE` or
      # past it.
      start_agent: probe(fn -> start_agent(10_000_000) end),
      start_agent_over: probe(fn -> start_agent(86_400_000_001) end),
      # Binding a handle it already holds under a new name creates no authority, and a lookup of a
      # path it was never given has nothing to resolve.
      bind_held: probe(&bind_held/0),
      lookup_unheld: probe(fn -> Redoubt.Namespace.lookup("/home/alice") end),
      # What it actually holds, written for the bench to read as the agent's starting authority:
      # `/project`, `/boot`, the system volume and its own `steward`; no `/net`, no console, no
      # sponsor connection.
      ns: Enum.map(Redoubt.Namespace.table(), fn {path, name, _h} -> {path, name} end)
    ]

    File.mkdir_p("/project/out")
    # Written and read back before the agent idles: the VM's end tears its connections down, and a
    # write still on its way would be lost with them (as `Redoubt.Agents.Echo` does).
    File.write(@output, inspect(record, limit: :infinity) <> "\n")
    _ = File.read(@output)
    # Idle until the lease ends: a lease's VM has no console to say more on, and its sponsor ends
    # it. A busy recursion, as Echo's live paths idle, never `Process.sleep/1`.
    spin()
  end

  defp spin, do: spin()

  # Each attempt's outcome, caught so one refusal cannot stop the rest: a thrown or raised error is
  # as good a refusal as a returned one.
  defp probe(fun) do
    fun.()
  rescue
    e -> {:raised, Exception.message(e)}
  catch
    kind, reason -> {:caught, kind, reason}
  end

  defp start_agent(lease) do
    with {:ok, c} <- Redoubt.Contexts.connection() do
      Redoubt.Wire.Client.Steward.start_agent(c, lease)
    end
  end

  # Bind the agent's own `/project` connection under `/x`: the call may succeed, but the handle is
  # the one it already held, so `/x` reaches nothing `/project` did not.
  defp bind_held do
    case Redoubt.Namespace.lookup("/project") do
      {:ok, handle, _rest} -> Redoubt.Namespace.bind("/x", handle)
      other -> other
    end
  end
end
