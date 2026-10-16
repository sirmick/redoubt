defmodule Redoubt.Shell.Notices do
  @moduledoc """
  The steward's notices to a session (docs/servers/steward.md, "Leases", "The powerbox and
  approvals" and "Failure and restart"; docs/userland/sessions.md, "What a session is told"): a
  session's VM is launched with the receive right of an endpoint, the named handle `notices`, on
  which the steward sends `lease_ended` when a lease of the session's principal ends, naming the
  lease and never why; `approval_waiting` when a request the session may approve is submitted,
  saying nothing of which: that is the approval terminal's alone; and `server_restarted` when a
  shared server the session's namespace reaches restarted, naming the server and the slot of the
  session's binding table that reached it. A listener process serves it for the VM's life. A
  lease's end prints `[lease NAME ended]` and an approval `[an approval is waiting]` through the
  shell's printer, so the line is drawn visibly whatever the name holds, as any output is. A
  server's restart is re-bound: the connection the session was launched with died with the
  server's old instance, so the listener asks the steward for the slot's connection again
  (`rebind`, on the session's own connection to the steward) and binds the reply's connection at
  the path the steward names, in place of the dead one; then one line says so,
  `[walfsd:data restarted: /home/alice re-bound]`, or why it was not.

  A VM launched with no `notices` handle (a host, a VM that is no session) starts no listener. A
  send the steward could not deliver in its bound is lost: the steward is never held by a session
  that does not read. Only a send on the steward's badge counts: code in the session holds the
  receive right and may send through its own endpoint, but under badge 0, which the VM cannot
  mint away from, so a forged notice prints nothing and re-binds nothing.
  """

  alias Redoubt.Contexts
  alias Redoubt.Shell.Printer
  alias Redoubt.Wire.Client.Steward
  alias Redoubt.Wire.Proto.Steward, as: P

  # beamlet's natives (docs/userland/beamlet.md, "Natives"): no module, so nothing to check at compile time.
  @compile {:no_warn_undefined, :redoubt}

  @handle "notices"

  @doc """
  Starts the listener on the session's `notices` endpoint, if the VM was given one: `{:ok, pid}`,
  or `{:error, reason}` with none (`:not_found`) or where it cannot be served.
  """
  @spec start() :: {:ok, pid()} | {:error, atom()}
  def start do
    with {:ok, endpoint} <- Redoubt.Namespace.handle(@handle) do
      {:ok, spawn(fn -> serve(endpoint) end)}
    end
  end

  defp serve(endpoint) do
    case :redoubt.serve(endpoint) do
      :ok -> loop()
      {:error, _reason} -> :ok
    end
  end

  defp loop do
    receive do
      # A send (no request to answer): the words and bytes the steward encoded, on the badge
      # the steward's send right carries. Code in the session holds the receive right and may
      # send through its own endpoint, but only under badge 0, which the VM cannot mint away
      # from: a forged notice does nothing.
      {:request, nil, badge, _account, _labels, {words, buffer, handles}} ->
        act(notice(badge, words, buffer || <<>>, handles))
        loop()

      # A call on this endpoint is nobody's: the steward only sends. It is left unanswered, and
      # the platform ends it at its deadline.
      {:request, _request, _badge, _account, _labels, _body} ->
        loop()

      _other ->
        loop()
    end
  end

  # The badge of the steward's send right on a session's notices endpoint (servers/steward,
  # `NOTICES`): the one badge a notice may arrive on.
  @notices 1

  @doc """
  What a send on the notices endpoint says: `{:lease_ended, name}`, `:approval_waiting` or
  `{:server_restarted, server, slot}` on the steward's badge; `nil` for any other message, or
  for one on any other badge: a send through the endpoint from code in the session carries badge
  0, so it cannot forge a notice.
  """
  @spec notice(non_neg_integer(), [non_neg_integer()], binary(), list()) ::
          {:lease_ended, String.t()}
          | :approval_waiting
          | {:server_restarted, String.t(), non_neg_integer()}
          | nil
  def notice(@notices, words, buffer, handles) do
    case P.decode(words, buffer, length(handles)) do
      {:ok, {:lease_ended, %{name: name}}} -> {:lease_ended, name}
      {:ok, {:approval_waiting, %{}}} -> :approval_waiting
      {:ok, {:server_restarted, %{server: server, slot: slot}}} -> {:server_restarted, server, slot}
      _other -> nil
    end
  end

  def notice(_badge, _words, _buffer, _handles), do: nil

  @doc """
  The line a notice sent on the steward's badge prints: `[lease NAME ended]` for `lease_ended`,
  `[an approval is waiting]` for `approval_waiting`; nothing for a `server_restarted` (its line
  says what the re-bind did), for any other message, or for one on any other badge.
  """
  @spec line(non_neg_integer(), [non_neg_integer()], binary(), list()) :: String.t() | nil
  def line(badge, words, buffer, handles) do
    case notice(badge, words, buffer, handles) do
      {:lease_ended, name} -> "[lease #{name} ended]"
      :approval_waiting -> "[an approval is waiting]"
      _other -> nil
    end
  end

  @doc """
  The line a `server_restarted` ends in, from what the re-bind of the slot did:
  `[SERVER restarted: PATH re-bound]`, or `[SERVER restarted: slot N not re-bound (REASON)]`.
  """
  @spec restarted_line(String.t(), non_neg_integer(), {:ok, String.t()} | {:error, term()}) :: String.t()
  def restarted_line(server, _slot, {:ok, at}), do: "[#{server} restarted: #{at} re-bound]"

  def restarted_line(server, slot, {:error, reason}),
    do: "[#{server} restarted: slot #{slot} not re-bound (#{inspect(reason)})]"

  defp act(nil), do: :ok
  defp act({:lease_ended, name}), do: Printer.text("[lease #{name} ended]")
  defp act(:approval_waiting), do: Printer.text("[an approval is waiting]")

  defp act({:server_restarted, server, slot}),
    do: Printer.text(restarted_line(server, slot, rebind(slot)))

  # The slot's connection again, from the steward, bound where the steward says: the reply's
  # handle is attached as a connection by the bind, and replaces the dead one at that prefix.
  defp rebind(slot) do
    with {:ok, steward} <- Contexts.connection(),
         {:ok, %{at: at, connection: connection}} <- Steward.rebind(steward, slot),
         :ok <- Redoubt.Namespace.bind(at, connection) do
      {:ok, at}
    end
  end
end
