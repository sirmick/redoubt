defmodule Redoubt.Jobs do
  @moduledoc """
  The session's jobs (docs/userland/shell.md, "Interrupting and killing jobs"): each pipeline the
  session runs, its owner process (`Redoubt.Pipeline`) and how it ended. Started at the first job,
  never at the prompt.

  A foreground job, a pipeline a line waits for, is here while it runs: its result goes to its
  line. A background job (`Redoubt.Job.start/2`) is here until its result is taken
  (`Redoubt.Job.await/2`), or until 16 newer background jobs have ended unread too: this
  server keeps that many results, up to 128 KiB each, and drops the earliest started past them,
  counting it (`dropped/0`). Killing a job is its owner's: it destroys its stages' budgets and
  nothing else, and this server only passes the word on and waits for the end.
  """

  use GenServer

  # The ended background jobs whose results are kept until taken, at most.
  @kept_ended 16

  @typedoc "How a job stands: running, or how its last stage ended."
  @type state :: :running | :exited | :faulted | :killed | :crashed

  # ---- what owners call ----

  @doc false
  # An owner's job, running: its number. `caller` is the process a foreground job's line runs in,
  # `nil` for a background job.
  @spec add(pid(), pid() | nil, String.t()) :: pos_integer()
  def add(owner, caller, command), do: GenServer.call(server(), {:add, owner, caller, command})

  @doc false
  # An owner's job has ended, every budget of it destroyed: `result` is what `await/2` gives.
  @spec ended(pos_integer(), map()) :: :ok
  def ended(id, result), do: GenServer.cast(server(), {:ended, id, result})

  # ---- what the shell calls ----

  @doc "The session's jobs: `%{id:, command:, state:, background:}`, oldest first."
  @spec list() :: [map()]
  def list, do: if(running?(), do: GenServer.call(server(), :list), else: [])

  @doc "How many ended background jobs were dropped unread, past the #{@kept_ended} kept."
  @spec dropped() :: non_neg_integer()
  def dropped, do: if(running?(), do: GenServer.call(server(), :dropped), else: 0)

  @doc "How the job `id` stands, or `:unknown`."
  @spec status(pos_integer()) :: state() | :unknown
  def status(id), do: if(running?(), do: GenServer.call(server(), {:status, id}), else: :unknown)

  @doc """
  Waits at most `timeout` for the job `id` to end, and gives its result, taking it: a job whose
  result is taken is no longer listed. `:timeout`, or `{:error, :unknown}`.
  """
  @spec await(pos_integer(), timeout()) :: map() | :timeout | {:error, :unknown}
  def await(id, timeout) do
    if running?(), do: call({:await, id, timeout}), else: {:error, :unknown}
  end

  @doc "Ends the job `id`, every budget of it, and gives its result; as `await/2` once it has ended."
  @spec kill(pos_integer()) :: map() | {:error, :unknown}
  def kill(id), do: if(running?(), do: call({:kill, id, :infinity}), else: {:error, :unknown})

  @doc """
  Waits at most `timeout` ms for every foreground job whose line has ended to end too: after an
  interrupt, their budgets are being destroyed. `:ok`, or `{:running, n}`.
  """
  @spec settle(non_neg_integer()) :: :ok | {:running, pos_integer()}
  def settle(timeout), do: if(running?(), do: call({:settle, timeout}), else: :ok)

  defp running?, do: Process.whereis(__MODULE__) != nil

  # A wait's bound is the server's: a waiter it gives up on is dropped, so the result is kept for
  # the next one.
  defp call(request) do
    GenServer.call(__MODULE__, request, :infinity)
  catch
    :exit, {:noproc, _} -> {:error, :unknown}
  end

  defp server do
    case GenServer.start(__MODULE__, nil, name: __MODULE__) do
      {:ok, pid} -> pid
      {:error, {:already_started, pid}} -> pid
    end
  end

  # ---- the server ----

  @impl true
  def init(nil), do: {:ok, %{next: 1, jobs: %{}, settling: [], dropped: 0}}

  @impl true
  def handle_call({:add, owner, caller, command}, _from, state) do
    job = %{
      owner: owner,
      monitor: Process.monitor(owner),
      caller: caller,
      command: command,
      result: nil,
      waiting: []
    }

    {:reply, state.next, %{state | next: state.next + 1, jobs: Map.put(state.jobs, state.next, job)}}
  end

  def handle_call(:list, _from, state) do
    list =
      state.jobs
      |> Enum.sort()
      |> Enum.map(fn {id, job} ->
        %{id: id, command: job.command, state: job_state(job), background: job.caller == nil}
      end)

    {:reply, list, state}
  end

  def handle_call(:dropped, _from, state), do: {:reply, state.dropped, state}

  def handle_call({:status, id}, _from, state) do
    {:reply, if(job = state.jobs[id], do: job_state(job), else: :unknown), state}
  end

  def handle_call({request, id, timeout}, from, state) when request in [:await, :kill] do
    case state.jobs[id] do
      nil ->
        {:reply, {:error, :unknown}, state}

      %{result: nil} = job ->
        if request == :kill, do: send(job.owner, {:redoubt_job, :kill})
        timer = if timeout != :infinity, do: Process.send_after(self(), {:gave_up, id, from}, timeout)
        {:noreply, put_in(state.jobs[id], %{job | waiting: [{from, timer} | job.waiting]})}

      %{result: result} ->
        {:reply, result, %{state | jobs: Map.delete(state.jobs, id)}}
    end
  end

  def handle_call({:settle, timeout}, from, state) do
    case orphans(state) do
      0 ->
        {:reply, :ok, state}

      _n ->
        timer = Process.send_after(self(), {:settled, from}, timeout)
        {:noreply, %{state | settling: [{from, timer} | state.settling]}}
    end
  end

  @impl true
  def handle_cast({:ended, id, result}, state), do: {:noreply, finish(state, id, result)}

  @impl true
  # An owner that ended without saying how: its `after` destroyed its budgets, if it got that far.
  def handle_info({:DOWN, monitor, :process, _owner, _reason}, state) do
    case Enum.find(state.jobs, fn {_id, job} -> job.monitor == monitor and job.result == nil end) do
      {id, _job} -> {:noreply, finish(state, id, %{endings: [], error: :crashed})}
      nil -> {:noreply, state}
    end
  end

  def handle_info({:gave_up, id, from}, state) do
    case state.jobs[id] do
      %{waiting: waiting} = job ->
        GenServer.reply(from, :timeout)
        waiting = Enum.reject(waiting, &match?({^from, _}, &1))
        {:noreply, put_in(state.jobs[id], %{job | waiting: waiting})}

      nil ->
        {:noreply, state}
    end
  end

  def handle_info({:settled, from}, state) do
    GenServer.reply(from, {:running, orphans(state)})
    {:noreply, %{state | settling: Enum.reject(state.settling, &match?({^from, _}, &1))}}
  end

  def handle_info(_other, state), do: {:noreply, state}

  # A job's result: to whoever waits on it; a foreground job's goes to its line, so it is dropped
  # here, and a background one's is kept until taken.
  defp finish(state, id, result) do
    case state.jobs[id] do
      nil ->
        state

      job ->
        Process.demonitor(job.monitor, [:flush])

        Enum.each(job.waiting, fn {from, timer} ->
          if timer, do: Process.cancel_timer(timer)
          GenServer.reply(from, result)
        end)

        jobs =
          if job.waiting != [] or job.caller != nil,
            do: Map.delete(state.jobs, id),
            else: Map.put(state.jobs, id, %{job | result: result, waiting: []})

        %{state | jobs: jobs} |> drop_unread() |> settled()
    end
  end

  # Past the ended background jobs kept, the earliest started is dropped, its output with it.
  defp drop_unread(state) do
    unread = for {id, %{caller: nil, result: result}} <- state.jobs, result != nil, do: id

    case length(unread) - @kept_ended do
      over when over > 0 ->
        drop = unread |> Enum.sort() |> Enum.take(over)
        %{state | jobs: Map.drop(state.jobs, drop), dropped: state.dropped + over}

      _ ->
        state
    end
  end

  # Those waiting on the foreground jobs of ended lines, once there are none.
  defp settled(%{settling: []} = state), do: state

  defp settled(state) do
    if orphans(state) == 0 do
      Enum.each(state.settling, fn {from, timer} ->
        Process.cancel_timer(timer)
        GenServer.reply(from, :ok)
      end)

      %{state | settling: []}
    else
      state
    end
  end

  defp orphans(state) do
    Enum.count(state.jobs, fn {_id, job} ->
      job.caller != nil and job.result == nil and not Process.alive?(job.caller)
    end)
  end

  defp job_state(%{result: nil}), do: :running
  defp job_state(%{result: result}), do: ended_state(result)

  @doc false
  # The state a result gives: its last stage's ending, `:killed` if the job was killed.
  @spec ended_state(map()) :: state()
  def ended_state(%{error: :crashed}), do: :crashed
  def ended_state(%{killed: true}), do: :killed
  def ended_state(%{endings: endings}) when endings != [], do: endings |> List.last() |> elem(0)
  def ended_state(_result), do: :crashed
end
