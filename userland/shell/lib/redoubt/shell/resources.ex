defmodule Redoubt.Shell.Resources do
  @moduledoc """
  Resource use, imported at the prompt (docs/userland/shell.md, "Resource use"): `free`,
  `uptime`, `ps` and `top`, whose screen is `Redoubt.Screen.Top`. They read and change nothing:
  the session's own budget, the named handle `budget`, through `Redoubt.Budget.usage/1`, which
  shows only what is the session's own, and the VM's own counts of its Erlang processes. A
  platform with no budgets, the host's, shows the VM's part alone.
  """

  use Redoubt.Commandlet, area: "Session"

  alias Redoubt.Util.Lines

  @page 4096
  @items [:registered_name, :memory, :reductions, :message_queue_len]

  @summary "Show the session's memory"
  @help """
  The session's pages: its budget's limit, how many are used and how many are free. Then what the
  VM's Erlang processes, binaries, atoms and ETS tables hold of them, in pages; its code and its
  own tables are the rest. Where there are no budgets, as on the host, the VM's part alone.
  """
  @examples [{"free()", "how much of the session's memory is left"}]
  defcommand free() do
    Lines.new(lines(:free, snapshot()))
  end

  @summary "Show how long the box and the session have been up"
  @help """
  How long since the box booted, by the kernel's clock, and since the session's VM started. Where
  there are no budgets, as on the host, the session's alone.
  """
  @examples [{"uptime()", "since the boot, and since this session started"}]
  defcommand uptime() do
    Lines.new(lines(:uptime, snapshot()))
  end

  @summary "List the session's processes"
  @help """
  The session's budget (its pages, processes and weight) and then the VM's Erlang processes, the
  largest first: each one's pid, registered name, memory in KiB, reductions and queued messages.
  Only the session's own work is shown. A native program it runs is in a budget carved from the
  session's, counted in its pages and processes.
  """
  @examples [{"ps()", "the session's processes, the largest first"}]
  defcommand ps() do
    Lines.new(lines(:ps, snapshot()))
  end

  @summary "Watch the session's resource use, on a screen"
  @help """
  What uptime, free and ps show, on a screen of its own, refreshed each second. Each process's
  reductions are those since the last refresh, and the busiest comes first. q or Esc leaves.
  """
  @examples [{"top()", "watch what the session is using"}]
  defcommand top() do
    Redoubt.Screen.run(Redoubt.Screen.Top, nil)
  end

  @doc false
  # What each command shows of `snapshot`, as lines of text; `last` is each process's reductions
  # at the refresh before, for `top`'s screen.
  @spec lines(:free | :uptime | :ps | {:top, map()}, map()) :: [String.t()]
  def lines(:free, snapshot), do: memory(snapshot)
  def lines(:uptime, snapshot), do: [uptime(snapshot)]
  def lines(:ps, snapshot), do: [budget(snapshot), "" | processes(snapshot.processes, nil)]

  def lines({:top, last}, snapshot),
    do: [uptime(snapshot) | memory(snapshot)] ++ [budget(snapshot), "" | processes(snapshot.processes, last)]

  @doc false
  # Everything the commands show, read once.
  @spec snapshot() :: map()
  def snapshot do
    %{
      usage: usage(),
      memory: Map.new(:erlang.memory()),
      processes: Enum.flat_map(Process.list(), &process/1),
      session_ms: elem(:erlang.statistics(:wall_clock), 0),
      clock_us: System.monotonic_time(:microsecond)
    }
  end

  # The session's own budget's use, or why there is none to read: `not_supported` on the host's
  # platform, and on the BEAM, which has no `redoubt` module.
  defp usage do
    with {:ok, budget} <- Redoubt.Budget.own(), do: Redoubt.Budget.usage(budget)
  rescue
    UndefinedFunctionError -> {:error, :not_supported}
  end

  # A process that ended since the list was taken is left out.
  defp process(pid) do
    case Process.info(pid, @items) do
      nil ->
        []

      info ->
        name =
          case info[:registered_name] do
            [] -> ""
            name -> name |> Atom.to_string() |> String.replace_prefix("Elixir.", "")
          end

        pid = List.to_string(:erlang.pid_to_list(pid))

        [
          %{
            pid: pid,
            name: name,
            memory: info[:memory],
            reductions: info[:reductions],
            queue: info[:message_queue_len]
          }
        ]
    end
  end

  # ---- as text ----

  defp memory(%{memory: m} = snapshot) do
    vm =
      "the VM: processes #{pages(m.processes)}, binaries #{pages(m.binary)}, " <>
        "atoms #{pages(m.atom)}, ETS #{pages(m.ets)} pages"

    case snapshot.usage do
      {:ok, %{pages: {limit, used}}} ->
        ["the session: #{limit} pages, #{used} used, #{limit - used} free", vm]

      _ ->
        [budget(snapshot), vm]
    end
  end

  defp pages(bytes), do: div(bytes + @page - 1, @page)

  defp uptime(%{usage: {:error, :not_supported}} = snapshot),
    do: "the session: up #{duration(snapshot.session_ms)}"

  # On Redoubt the VM's monotonic clock is the kernel's, which counts from the boot.
  defp uptime(snapshot),
    do:
      "the box: up #{duration(div(snapshot.clock_us, 1000))}; the session: up #{duration(snapshot.session_ms)}"

  defp duration(ms) do
    s = div(ms, 1000)
    clock = :io_lib.format("~2..0B:~2..0B:~2..0B", [rem(div(s, 3600), 24), rem(div(s, 60), 60), rem(s, 60)])
    days = div(s, 86_400)
    if days > 0, do: "#{days} d #{clock}", else: List.to_string(clock)
  end

  defp budget(%{usage: {:ok, %{pages: {pl, pu}, processes: {nl, nu}, weight: {wl, wc}}}}),
    do: "the budget: #{pu} of #{pl} pages, #{nu} of #{nl} processes, weight #{wl} (#{wc} carved)"

  defp budget(%{usage: {:error, :not_supported}}), do: "the budget: none on this platform"
  defp budget(%{usage: {:error, reason}}), do: "the budget: not read (#{reason})"

  # The processes, a row each under a header: the largest first, or, with the reductions at the
  # last refresh, the busiest since then.
  defp processes(processes, last) do
    rows =
      for p <- processes do
        reductions = if last, do: p.reductions - Map.get(last, p.pid, p.reductions), else: p.reductions
        {p, reductions}
      end

    sorted = Enum.sort_by(rows, fn {p, r} -> if last, do: {-r, -p.memory}, else: {-p.memory, -r} end)
    header = row("PID", "NAME", "KIB", if(last, do: "REDS/S", else: "REDUCTIONS"), "QUEUE")
    [header | Enum.map(sorted, fn {p, r} -> row(p.pid, p.name, div(p.memory + 1023, 1024), r, p.queue) end)]
  end

  defp row(pid, name, kib, reductions, queue) do
    String.pad_trailing(pid, 12) <>
      String.pad_trailing(name, 24) <>
      String.pad_leading(to_string(kib), 8) <>
      String.pad_leading(to_string(reductions), 12) <> String.pad_leading(to_string(queue), 7)
  end
end
