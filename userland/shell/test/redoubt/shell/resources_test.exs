defmodule Redoubt.Shell.ResourcesTest do
  use ExUnit.Case, async: false

  import Redoubt.Shell.Resources, only: [free: 0, uptime: 0, ps: 0]

  alias Redoubt.Shell.Resources
  alias Redoubt.Test.Terminal

  # A session's budget as budget_usage reads it, and the VM's own counts, as a snapshot has them.
  defp snapshot(usage, processes \\ []) do
    %{
      usage: usage,
      memory: %{processes: 3 * 4096, binary: 1, atom: 4096 + 1, ets: 0},
      processes: processes,
      session_ms: 5_000,
      clock_us: (86_400 + 3_661) * 1_000_000
    }
  end

  @usage %{pages: {11_904, 5_123}, processes: {64, 3}, weight: {10, 4}}

  defp p(pid, memory, reductions), do: %{pid: pid, name: "", memory: memory, reductions: reductions, queue: 0}

  test "on a platform with no budgets, as here, each shows the VM's part and says so" do
    [budget, vm] = Enum.to_list(free())
    assert budget == "the budget: none on this platform"
    assert vm =~ ~r/^the VM: processes \d+, binaries \d+, atoms \d+, ETS \d+ pages$/

    assert [up] = Enum.to_list(uptime())
    assert up =~ ~r/^the session: up \d\d:\d\d:\d\d$/

    [budget, "", header | rows] = Enum.to_list(ps())
    assert budget == "the budget: none on this platform"
    assert header =~ ~r/^PID +NAME +KIB +REDUCTIONS +QUEUE$/
    me = List.to_string(:erlang.pid_to_list(self()))
    assert Enum.any?(rows, &String.starts_with?(&1, me <> " ")), "the caller is one of the VM's processes"
  end

  test "with a budget, each shows the session's own use, and the box's clock beside the session's" do
    s = snapshot({:ok, @usage})

    assert Resources.lines(:free, s) == [
             "the session: 11904 pages, 5123 used, 6781 free",
             "the VM: processes 3, binaries 1, atoms 2, ETS 0 pages"
           ]

    assert Resources.lines(:uptime, s) == ["the box: up 1 d 01:01:01; the session: up 00:00:05"]

    assert hd(Resources.lines(:ps, s)) ==
             "the budget: 5123 of 11904 pages, 3 of 64 processes, weight 10 (4 carved)"
  end

  test "a budget the kernel refuses to read is named, not shown" do
    s = snapshot({:error, :label_denied})
    assert hd(Resources.lines(:free, s)) == "the budget: not read (label_denied)"
    assert hd(Resources.lines(:ps, s)) == "the budget: not read (label_denied)"
  end

  test "ps lists the largest first; top the busiest since the last refresh" do
    s = snapshot({:ok, @usage}, [p("<0.1.0>", 1024, 900), p("<0.2.0>", 8192, 100), p("<0.3.0>", 2048, 50)])

    [_budget, "", _header | rows] = Resources.lines(:ps, s)
    assert Enum.map(rows, &binary_part(&1, 0, 7)) == ["<0.2.0>", "<0.3.0>", "<0.1.0>"]
    assert hd(rows) =~ ~r/^<0\.2\.0> +8 +100 +0$/

    last = %{"<0.1.0>" => 890, "<0.2.0>" => 100, "<0.3.0>" => 0}
    lines = Resources.lines({:top, last}, s)
    [header | rows] = Enum.drop_while(lines, &(not String.starts_with?(&1, "PID")))
    assert header =~ "REDS/S"
    assert Enum.map(rows, &binary_part(&1, 0, 7)) == ["<0.3.0>", "<0.1.0>", "<0.2.0>"]
    assert hd(lines) == "the box: up 1 d 01:01:01; the session: up 00:00:05"
  end

  describe "top, on a screen" do
    unless Redoubt.Term.Buffer.available?() do
      @describetag skip: "the screen buffer is beamlet's natives, which the BEAM does not have"
    end

    test "shows the session's use and its processes, and q leaves" do
      test = self()

      driver =
        spawn_link(fn ->
          Redoubt.Shell.Driver.run(
            input: :messages,
            output: fn bytes -> send(test, {:drawn, IO.iodata_to_binary(bytes)}) end,
            size: fn -> {80, 20} end,
            shell: {Redoubt.Shell, :start_link, [[banner: false]]}
          )
        end)

      t = screen(Terminal.new(80, 20))
      send(driver, {:beamlet_console, "top()\r"})
      t = screen(t, &(Terminal.text(&1) =~ "q or Esc leave"))
      assert Terminal.alternate?(t)
      assert Terminal.text(t) =~ "the budget: none on this platform"
      assert Terminal.text(t) =~ ~r/PID +NAME +KIB +REDS\/S +QUEUE/

      send(driver, {:beamlet_console, "q"})
      t = screen(t, &(not Terminal.alternate?(&1)))
      refute Terminal.alternate?(t)
      assert Terminal.text(t) =~ ":ok"

      send(driver, {:beamlet_console, "exit\r"})
      ref = Process.monitor(driver)
      assert_receive {:DOWN, ^ref, :process, ^driver, :normal}, 5_000
    end
  end

  # The terminal once the shell has drawn what it will, or (with `done`) once it shows that.
  defp screen(terminal, done \\ fn _terminal -> true end) do
    receive do
      {:drawn, bytes} -> terminal |> Terminal.feed(bytes) |> screen(done)
    after
      if(done.(terminal), do: 300, else: 10_000) -> terminal
    end
  end
end
