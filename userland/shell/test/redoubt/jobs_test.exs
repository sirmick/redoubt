defmodule Redoubt.JobsTest do
  # The session's job table is one named server: run alone.
  use ExUnit.Case, async: false

  alias Redoubt.{Job, Jobs}

  # A stand-in for a pipeline's owner (`Redoubt.Pipeline`): registered as a job, it ends when
  # told to, or when killed, saying how as an owner does once every budget is destroyed.
  defp owner(caller, command \\ "stage") do
    test = self()

    pid =
      spawn(fn ->
        id = Jobs.add(self(), caller, command)
        send(test, {:added, self(), id})

        receive do
          {:redoubt_job, :kill} -> Jobs.ended(id, %{endings: [{:killed, 0}], killed: true})
          {:end, result} -> Jobs.ended(id, result)
        end
      end)

    assert_receive {:added, ^pid, id}
    {pid, id}
  end

  setup do
    on_exit(fn -> if pid = Process.whereis(Jobs), do: GenServer.stop(pid) end)
  end

  test "a background job is listed until its result is taken, and kill ends it and only it" do
    {a, a_id} = owner(nil, "yes | count")
    {b, b_id} = owner(nil, "gen 10")

    assert [%{id: ^a_id, state: :running, background: true}, %{id: ^b_id, state: :running}] = Jobs.list()
    assert %{killed: true} = Jobs.kill(a_id)
    refute Process.alive?(a)
    assert Jobs.status(a_id) == :unknown
    assert Jobs.status(b_id) == :running

    send(b, {:end, %{endings: [{:exited, 0}], killed: false, stdout: "out"}})
    Process.sleep(50)
    assert Jobs.status(b_id) == :exited
    assert [%{id: ^b_id, state: :exited}] = Jobs.list()
    assert %{stdout: "out"} = Jobs.await(b_id, 1_000)
    assert Jobs.list() == []
    assert Jobs.await(b_id, 1_000) == {:error, :unknown}
  end

  test "await gives up at its timeout, and the job runs on" do
    {pid, id} = owner(nil)
    assert Jobs.await(id, 50) == :timeout
    assert Jobs.status(id) == :running
    send(pid, {:end, %{endings: [{:faulted, 3}], killed: false}})
    assert %{endings: [{:faulted, 3}]} = Jobs.await(id, 1_000)
  end

  test "a job that ends after an await gave up keeps its result for the next" do
    {pid, id} = owner(nil)
    assert Jobs.await(id, 50) == :timeout
    send(pid, {:end, %{endings: [{:exited, 0}], killed: false, stdout: "late"}})
    Process.sleep(50)
    assert Jobs.status(id) == :exited
    assert %{stdout: "late"} = Jobs.await(id, 1_000)
  end

  test "past 16 ended background jobs unread, the earliest is dropped and counted" do
    owners = for n <- 1..17, do: owner(nil, "gen #{n}")

    for {pid, _id} <- owners,
        do: send(pid, {:end, %{endings: [{:exited, 0}], killed: false, stdout: "out"}})

    Process.sleep(100)
    [{_first, first} | rest] = owners
    assert Enum.map(Jobs.list(), & &1.id) == Enum.map(rest, &elem(&1, 1))
    assert Jobs.dropped() == 1
    assert Jobs.await(first, 100) == {:error, :unknown}
    assert %{stdout: "out"} = Jobs.await(elem(hd(rest), 1), 100)
  end

  test "a foreground job's result is its line's: it leaves the table when it ends" do
    {pid, id} = owner(self())
    assert [%{id: ^id, background: false}] = Jobs.list()
    send(pid, {:end, %{endings: [{:exited, 0}], killed: false}})
    Process.sleep(50)
    assert Jobs.list() == []
  end

  test "an owner that dies without saying how is listed as crashed" do
    {pid, id} = owner(nil)
    Process.exit(pid, :kill)
    Process.sleep(50)
    assert Jobs.status(id) == :crashed
  end

  test "settle waits for the foreground jobs of ended lines, and only for its bound" do
    assert Jobs.settle(10) == :ok

    line = spawn(fn -> receive do: (:never -> :ok) end)
    {pid, _id} = owner(line)
    {_background, _} = owner(nil)
    {_live_line_job, _} = owner(self())

    # The line is alive: nothing to wait for.
    assert Jobs.settle(10) == :ok
    Process.exit(line, :kill)
    assert Jobs.settle(50) == {:running, 1}

    spawn(fn ->
      Process.sleep(50)
      send(pid, {:end, %{endings: [{:killed, 0}], killed: false}})
    end)

    assert Jobs.settle(2_000) == :ok
  end

  test "with no job ever started, nothing is started to answer" do
    assert Jobs.list() == []
    assert Jobs.status(1) == :unknown
    assert Jobs.dropped() == 0
    assert Jobs.settle(10) == :ok
    assert Process.whereis(Jobs) == nil
  end

  test "a background job never reads the console, and one it cannot start is refused by name" do
    assert Job.start([{"cat", []}], input: :console) == {:error, :console}
    # Read from /boot, which a host has not.
    assert Job.start([{"no-such-program", []}]) == {:error, :not_found}
    assert Job.start([]) == {:error, :empty_stage}
  end
end
