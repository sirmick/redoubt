defmodule Redoubt.Agents.Echo do
  @moduledoc """
  The agent of the steward's lease cases (docs/servers/steward.md, "Leases"; docs/userland/agents.md,
  "Leases"): the module a lease's VM runs, in a namespace holding the sponsor's `~/project` at
  `/project` and the system volume, nothing else. It reads `/project/in` and does what the line
  says:

  - `echo`: writes the input back to `/project/out/result` and exits, so the sponsor sees the
    lease end on its own;
  - `keep TEXT`: writes `TEXT` to `/project/out/result` and waits for ever: the echo path kept
    alive for `beamlet-agent-footprint` to measure, an agent that has done its work and idles;
  - `spin`: loops for ever, so only its deadline or the sponsor's `end_lease` ends it;
  - `sub MS`: carves a sub-budget of its own with a deadline `MS` milliseconds away, longer than
    its lease's, launches `piped` from `/boot` in it as the session's pipes do (`Redoubt.Pipes`),
    a server that lives until its budget ends, and loops: the lease's end takes the sub-budget
    and its process with it, whatever their own deadline (R39).

  An input it cannot read, or does not understand, is written to `/project/out/result` as the
  error it was, and the agent waits for its lease to end: it has no console to say more on, and
  its sponsor ends it or its deadline does.
  """

  @input "/project/in"
  @output "/project/out/result"
  @piped "/boot/piped"

  @doc "beamlet's entry point (`beamlet ... Elixir.Redoubt.Agents.Echo`)."
  def start do
    case File.read(@input) do
      {:ok, text} -> run(String.trim(text))
      {:error, reason} -> wait("in: #{inspect(reason)}")
    end
  end

  defp run("echo" <> _rest = text), do: finish(text)

  # The echo path, then idle: what `beamlet-agent-footprint` measures, an agent that has done
  # its work and waits.
  defp run("keep " <> text) do
    finish(text)
    Process.sleep(:infinity)
  end

  defp run("spin"), do: spin()

  defp run("sub " <> ms) do
    with {ms, ""} <- Integer.parse(ms),
         {:ok, budget} <- Redoubt.Budget.carve(pages: 768, processes: 1, weight: 1, deadline_ms: ms),
         {:ok, image} <- File.read(@piped),
         {:ok, _job, _server} <-
           Redoubt.Process.launch(%{image: image, budget: budget, serve: "serve", args: ["buckets=2"]}) do
      spin()
    else
      other -> wait("sub: #{inspect(other)}")
    end
  end

  defp run(other), do: wait("unknown: #{other}")

  defp spin, do: spin()

  # The result written, if it can be; then the lease's end.
  defp wait(text) do
    finish(text)
    spin()
  end

  # The result, written and read back before the agent returns: the VM's end tears its
  # connections down, and a write still on its way would be lost with them.
  defp finish(text) do
    File.mkdir_p(Path.dirname(@output))
    File.write(@output, text <> "\n")
    _ = File.read(@output)
    # Said on the console if the VM has one: a lease's has none, the footprint case's does, and
    # that case waits for this line before it measures.
    IO.puts("agent: result written")
    :ok
  end
end
