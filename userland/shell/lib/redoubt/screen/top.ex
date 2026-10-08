defmodule Redoubt.Screen.Top do
  @moduledoc """
  A screen: `top()`'s (`Redoubt.Shell.Resources`). What `uptime`, `free` and `ps` show, refreshed
  each second, each process's reductions those since the last refresh and the busiest first. q or
  Esc leaves.
  """

  @behaviour Redoubt.Screen

  alias Redoubt.Screen.{Layout, Widgets}
  alias Redoubt.Shell.Resources
  alias Redoubt.Term.Buffer

  @refresh_ms 1000
  @hint "q or Esc leave · refreshed each second"

  @impl Redoubt.Screen
  def init(nil) do
    Process.send_after(self(), :refresh, @refresh_ms)
    %{snapshot: Resources.snapshot(), last: %{}}
  end

  @impl Redoubt.Screen
  def update({:key, :esc, _mods}, _state), do: {:halt, :ok}
  def update({:key, "q", []}, _state), do: {:halt, :ok}

  def update(:refresh, %{snapshot: before}) do
    Process.send_after(self(), :refresh, @refresh_ms)
    {:cont, %{snapshot: Resources.snapshot(), last: Map.new(before.processes, &{&1.pid, &1.reductions})}}
  end

  def update(_event, state), do: {:cont, state}

  @impl Redoubt.Screen
  def view(state, buffer, {cols, rows}) do
    [{x, y, w, h}, status] = Layout.split({0, 0, cols, rows}, :rows, [:rest, {:fixed, 1}])

    Resources.lines({:top, state.last}, state.snapshot)
    |> Enum.take(h)
    |> Enum.with_index()
    |> Enum.each(fn {line, i} -> Widgets.label(buffer, {x, y + i, w, 1}, line, Buffer.plain()) end)

    Widgets.status(buffer, status, @hint)
  end
end
