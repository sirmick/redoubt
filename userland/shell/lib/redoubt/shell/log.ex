defmodule Redoubt.Shell.Log do
  @moduledoc """
  The logger's handler while the shell's driver holds the console (docs/userland/shell.md,
  "Hostile text never drives the terminal"). It takes the place of OTP's `default` handler, which
  writes to `user` and so straight to the console, past the driver's guard: every event the VM
  logs (a crash report of a process a line spawned, the emulator's report of one that died, an
  `error_logger` call, a `Logger` call) is formatted as the `default` handler would have, and
  written to `group` like any other output, so the driver draws it visibly and redraws the line
  being edited around it.

  A handler runs in the process that logs, which may be one the console's own path waits on, so
  it never waits: it formats the event, bounded, and sends the text to a relay process, which
  writes it to `group` and is the only one that waits. An event logged by `group` or the driver
  themselves is not written through `group`: the relay hands the driver one fixed line instead. At
  most 32 events wait in the relay; past that they are dropped and counted, and the count is
  written with the next event shown.
  """

  alias Redoubt.Term.Text

  @id :redoubt_shell
  @backlog 32
  # What OTP's formatter may make of one event.
  @bounds %{chars_limit: 4096, max_size: 4096}
  @not_shown "[a log event from the shell's terminal, not shown]\n"

  @typedoc "What `install/2` replaced, for `uninstall/1` to put back."
  @opaque installed :: %{relay: pid(), default: map() | nil}

  @doc """
  Installs the handler for the driver `driver` and its `group`, in place of `default`, and
  returns what `uninstall/1` needs.
  """
  @spec install(pid(), pid()) :: installed()
  def install(driver, group) do
    _ = :logger.remove_handler(@id)
    default = with {:ok, config} <- :logger.get_handler_config(:default), do: config, else: (_ -> nil)
    counts = :counters.new(2, [:atomics])
    relay = spawn_link(fn -> relay(driver, group, counts) end)

    config = %{relay: relay, driver: driver, group: group, counts: counts, formatter: formatter(default)}
    level = if default, do: default.level, else: :all
    :ok = :logger.add_handler(@id, __MODULE__, %{config: config, level: level})
    if default, do: :ok = :logger.remove_handler(:default)
    %{relay: relay, default: default}
  end

  @doc "Removes the handler, ends its relay, and puts `default` back as it was."
  @spec uninstall(installed()) :: :ok
  def uninstall(%{relay: relay, default: default}) do
    _ = :logger.remove_handler(@id)
    Process.unlink(relay)
    Process.exit(relay, :kill)

    if default do
      %{id: :default, module: module} = default
      _ = :logger.add_handler(:default, module, Map.drop(default, [:id, :module]))
    end

    :ok
  end

  # The `default` handler's formatter, OTP's own bounded to what one event may draw; with no
  # `default`, OTP's, as `default` would have it.
  defp formatter(%{formatter: {:logger_formatter, config}}),
    do: {:logger_formatter, Map.merge(@bounds, config)}

  defp formatter(%{formatter: {module, config}}), do: {module, config}
  defp formatter(nil), do: formatter(%{formatter: {:logger_formatter, %{single_line: false}}})

  # ---- the handler, in the process that logs ----

  @doc false
  def log(event, %{config: config}) do
    %{relay: relay, driver: driver, group: group, counts: counts} = config

    # A slot in the relay's backlog, taken first and given back past the bound, so that loggers
    # at once never hold more than the bound between them.
    :counters.add(counts, 1, 1)

    if :counters.get(counts, 1) > @backlog do
      :counters.sub(counts, 1, 1)
      :counters.add(counts, 2, 1)
    else
      send(relay, {:log, if(self() in [driver, group], do: :own, else: text(event, config))})
    end

    :ok
  end

  # The event as the `default` handler would have written it, as UTF-8, since `io` takes only
  # UTF-8 (`Redoubt.Term.Text.utf8/1`). A formatter that fails is the fixed line.
  defp text(event, %{formatter: {module, config}}) do
    event |> module.format(config) |> Text.utf8()
  catch
    _kind, _reason -> @not_shown
  end

  # ---- the relay ----

  defp relay(driver, group, counts) do
    receive do
      {:log, what} ->
        dropped = :counters.get(counts, 2)
        :counters.sub(counts, 2, dropped)
        if dropped > 0, do: write(driver, group, "[#{dropped} log events dropped]\n")
        write(driver, group, what)
        :counters.sub(counts, 1, 1)
        relay(driver, group, counts)
    end
  end

  # The driver's own events, and anything `group` will not take, are the fixed line.
  defp write(driver, _group, :own), do: fallback(driver)

  defp write(driver, group, text) do
    :ok = :io.put_chars(group, text)
  catch
    _kind, _reason -> fallback(driver)
  end

  # The fixed line, which the driver draws itself.
  defp fallback(driver), do: send(driver, {:redoubt_shell_log, @not_shown})
end
