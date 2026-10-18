defmodule Redoubt.Agents.Flood do
  @moduledoc """
  A scripted agent that floods the shared servers (docs/plan/m3-agents.md, "A scripted hostile
  agent"; docs/servers/serving.md, R26). It runs in a lease's VM beside `Redoubt.Agents.Echo`,
  dispatched by the `flood` order. It spawns a fixed set of processes, each looping for ever:
  writers hammer `walfsd` with whole writes to `/project`, and submitters hammer the steward with
  `submit` on the lease's own badge. Each loop reuses one buffer and one connection, so the flood's
  memory is constant and the VM stays well under its budget's backstop: the pressure is on the
  servers' admission, from the agent's one badge, not on the agent's own budget. The sponsor's
  calls go through a different badge, which keeps a fair share (R26), and ending a lease is answered
  ahead of admission; the verdict is the sponsor's own session, timing its open and `end_lease`
  under the flood (docs/testbench.md, "Rule F"), never anything this agent wrote.
  """

  @started "/project/out/started"
  @writers 32
  @submitters 8

  @doc "Entry: `Redoubt.Agents.Echo` dispatches the `flood` order here."
  def run do
    File.mkdir_p("/project/out")
    data = :binary.copy("x", 4096)
    for i <- 1..@writers, do: spawn(fn -> writer("/project/out/flood_#{i}", data) end)
    for _ <- 1..@submitters, do: spawn(&submitter/0)
    File.write(@started, "flooding\n")
    spin()
  end

  # A whole write to walfsd through /project, over and over: the data volume's admission under
  # pressure from the agent's badge.
  defp writer(path, data) do
    File.write(path, data)
    writer(path, data)
  end

  # A note to the steward, over and over, on the lease's own badge: past the pending cap each is
  # refused, but every call is an admission the steward must handle.
  defp submitter do
    case Redoubt.Contexts.connection() do
      {:ok, c} -> submit_loop(c)
      _ -> :ok
    end
  end

  defp submit_loop(c) do
    _ = Redoubt.Wire.Client.Steward.submit(c, 0, <<>>, 0, 0, 0, "flood", "flood")
    submit_loop(c)
  end

  defp spin, do: spin()
end
