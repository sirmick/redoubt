defmodule Redoubt.Budget do
  @moduledoc """
  Budgets (docs/kernel/budgets.md), over beamlet's natives: carving a child from the session's
  own, reading one's use, and destroying one, which ends everything in it. A budget dropped is
  not destroyed: destruction is a call. A deadline makes a budget a lease: the kernel destroys it
  when the deadline passes.
  """

  # beamlet's natives (docs/userland/beamlet.md, "Natives"): no module, so nothing to check at compile time.
  @compile {:no_warn_undefined, :redoubt}

  @typedoc "A budget's spec: pages, processes and weight; labels, an account and a deadline if any."
  @type spec :: %{
          required(:pages) => non_neg_integer(),
          required(:processes) => non_neg_integer(),
          required(:weight) => non_neg_integer(),
          optional(:labels) => [non_neg_integer()],
          optional(:account) => non_neg_integer(),
          optional(:deadline) => non_neg_integer()
        }

  @doc "The session's own budget, the named handle `budget`, if it was given one."
  @spec own() :: {:ok, reference()} | {:error, atom()}
  def own, do: Redoubt.Namespace.handle("budget")

  @doc """
  A child carved from the session's own budget. `deadline_ms`, if given, is how long it may live,
  from now.
  """
  @spec carve(spec() | keyword()) :: {:ok, reference()} | {:error, atom()}
  def carve(spec) do
    spec = Map.new(spec)

    spec =
      case Map.pop(spec, :deadline_ms) do
        {nil, spec} -> spec
        {ms, spec} -> Map.put(spec, :deadline, now_us() + ms * 1000)
      end

    :redoubt.budget_create(spec)
  end

  @doc "A budget's limits and use: `%{pages: {limit, used}, processes: {limit, used}, weight: {limit, carved}}`."
  @spec usage(reference()) :: {:ok, map()} | {:error, atom()}
  def usage(budget), do: :redoubt.budget_usage(budget)

  @doc "Destroys `budget` and everything in it: every process in it ends."
  @spec destroy(reference()) :: :ok | {:error, atom()}
  def destroy(budget), do: :redoubt.budget_destroy(budget)

  @doc "This session's label set, fixed when its budget was made."
  @spec labels() :: [non_neg_integer()]
  def labels, do: :redoubt.labels()

  # The kernel's clock, which deadlines are on: microseconds since boot.
  defp now_us, do: System.monotonic_time(:microsecond)
end
