defmodule Redoubt.Shell.LogTest do
  use ExUnit.Case, async: true

  # A relay that never takes what it is sent, so the backlog only fills.
  defp stuck_relay, do: spawn(fn -> Process.sleep(:infinity) end)

  test "loggers at once hold exactly the backlog in the relay between them, and the rest are counted" do
    relay = stuck_relay()
    counts = :counters.new(2, [:atomics])

    config = %{
      relay: relay,
      driver: self(),
      group: self(),
      counts: counts,
      formatter: {:logger_formatter, %{}}
    }

    event = %{level: :error, msg: {:string, "x"}, meta: %{time: :logger.timestamp()}}

    1..16
    |> Enum.map(fn _ ->
      Task.async(fn -> for _ <- 1..20, do: Redoubt.Shell.Log.log(event, %{config: config}) end)
    end)
    |> Task.await_many()

    assert Process.info(relay, :message_queue_len) == {:message_queue_len, 32}
    assert :counters.get(counts, 1) == 32
    assert :counters.get(counts, 2) == 16 * 20 - 32
    Process.exit(relay, :kill)
  end
end
