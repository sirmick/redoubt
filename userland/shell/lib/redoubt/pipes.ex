defmodule Redoubt.Pipes do
  @moduledoc """
  The session's pipes' server, `piped` (docs/servers/piped.md): started from `/boot/piped` when a
  pipeline needs it and none is running, in a budget carved from the session's, and bound at
  `/dev/pipe`, where the session makes and reads its pipes. Nothing of it exists at the prompt.
  Each process that opens it holds it until it lets go or ends; when the last does, its budget is
  destroyed, so its pages go back to the session, and the next pipeline starts another.

  One process holds it, so that its connection and budget outlive the line that started it: the
  launch's end arrives as a message to the process that launched it, which is this one.
  """

  use GenServer

  # beamlet's natives (docs/userland/beamlet.md, "Natives"): no module, so nothing to check at compile time.
  @compile {:no_warn_undefined, :redoubt}

  alias Redoubt.{Budget, Namespace}

  @prefix "/dev/pipe"
  # Its image, about 52 pages on rv64, and the most its clients may make it hold: the parked calls'
  # lends at worst and their records, 552 pages, beside 32 one-page pipes (docs/servers/piped.md).
  @budget %{pages: 768, processes: 1, weight: 1}
  # The session and its stages are one client, of one account and label set: one bucket, and the
  # fewest admission allows is two.
  @args ["buckets=2"]
  # How often, and how far apart, a bind is tried while piped starts: each try waits up to a
  # second for it to answer.
  @bind_tries 5
  @bind_wait_ms 50

  @doc "Where the session's pipes are, once `open/1` has started their server."
  @spec prefix() :: String.t()
  def prefix, do: @prefix

  @doc """
  The session's connection to `piped`, starting it if it is not running, and `count` names for
  pipes no other job of the session's has used; the calling process holds `piped` until it calls
  `release/0` or ends. `{:error, name}` if it cannot be started.
  """
  @spec open(pos_integer()) :: {:ok, reference(), [String.t()]} | {:error, atom()}
  def open(count) when is_integer(count) and count > 0 do
    GenServer.call(server(), {:open, count}, :infinity)
  end

  @doc """
  Lets go of `piped` for the calling process: when no process holds it, its budget is destroyed
  before this returns.
  """
  @spec release() :: :ok
  def release, do: GenServer.call(server(), :release, :infinity)

  @doc """
  What `piped`'s budget holds now, as `Redoubt.Budget.usage/1` reads it, or `{:error, :not_running}`
  before a pipeline has started it.
  """
  @spec usage() :: {:ok, map()} | {:error, atom()}
  def usage, do: GenServer.call(server(), :usage)

  defp server do
    case GenServer.start(__MODULE__, nil, name: __MODULE__) do
      {:ok, pid} -> pid
      {:error, {:already_started, pid}} -> pid
    end
  end

  @impl true
  def init(nil), do: {:ok, %{conn: nil, job: nil, budget: nil, next: 1, users: %{}}}

  @impl true
  def handle_call({:open, count}, {pid, _tag}, state) do
    case running(state) do
      {:ok, state} ->
        names = Enum.map(state.next..(state.next + count - 1)//1, &"p#{&1}")
        users = Map.put_new_lazy(state.users, pid, fn -> Process.monitor(pid) end)
        {:reply, {:ok, state.conn, names}, %{state | next: state.next + count, users: users}}

      {:error, _} = error ->
        {:reply, error, state}
    end
  end

  def handle_call(:release, {pid, _tag}, state) do
    case Map.pop(state.users, pid) do
      {nil, _users} ->
        {:reply, :ok, state}

      {monitor, users} ->
        Process.demonitor(monitor, [:flush])
        {:reply, :ok, unused(%{state | users: users})}
    end
  end

  def handle_call(:usage, _from, %{budget: nil} = state), do: {:reply, {:error, :not_running}, state}
  def handle_call(:usage, _from, state), do: {:reply, Budget.usage(state.budget), state}

  # piped has ended: what it held went with its budget, and the next pipeline starts another.
  @impl true
  def handle_info({:exit, job, _cause, _code}, %{job: job} = state) do
    Budget.destroy(state.budget)
    {:noreply, %{state | conn: nil, job: nil, budget: nil}}
  end

  # A holder ended without letting go.
  def handle_info({:DOWN, _monitor, :process, pid, _reason}, state) do
    {:noreply, unused(%{state | users: Map.delete(state.users, pid)})}
  end

  def handle_info(_other, state), do: {:noreply, state}

  # Nobody holds piped: its budget goes, and piped with it. Its exit notice, when it comes, is
  # for a job no longer this one's.
  defp unused(%{users: users, budget: budget} = state) when map_size(users) == 0 and budget != nil do
    Budget.destroy(budget)
    %{state | conn: nil, job: nil, budget: nil}
  end

  defp unused(state), do: state

  defp running(%{conn: conn} = state) when conn != nil, do: {:ok, state}

  defp running(state) do
    with {:ok, image} <- read("/boot/piped"),
         {:ok, budget} <- Budget.carve(@budget) do
      case :redoubt.launch(%{image: image, budget: budget, serve: "serve", args: @args}) do
        {:ok, job, conn} ->
          case bind(conn, @bind_tries) do
            :ok ->
              {:ok, %{state | conn: conn, job: job, budget: budget}}

            {:error, _} = error ->
              Budget.destroy(budget)
              error
          end

        {:error, _} = error ->
          Budget.destroy(budget)
          error
      end
    end
  end

  # The bind attaches: it waits for piped to answer, which it does once it has started.
  defp bind(conn, tries) do
    case Namespace.bind(@prefix, conn) do
      {:error, :not_a_connection} when tries > 1 ->
        Process.sleep(@bind_wait_ms)
        bind(conn, tries - 1)

      result ->
        result
    end
  end

  defp read(path) do
    case File.read(path) do
      {:ok, bytes} -> {:ok, bytes}
      {:error, :enoent} -> {:error, :not_found}
      {:error, _} -> {:error, :refused}
    end
  end
end
