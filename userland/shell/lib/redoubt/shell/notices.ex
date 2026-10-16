defmodule Redoubt.Shell.Notices do
  @moduledoc """
  The steward's notices to a session (docs/servers/steward.md, "Leases"; docs/userland/sessions.md,
  "What a session is told"): a session's VM is launched with the receive right of an endpoint,
  the named handle `notices`, on which the steward sends `lease_ended` when a lease of the
  session's principal ends, naming the lease and never why, and `approval_waiting` when a request
  the session may approve is submitted (docs/servers/steward.md, "The powerbox and approvals"),
  saying nothing of which: that is the approval terminal's alone. A listener process serves it
  for the VM's life and prints each notice as `[lease NAME ended]` or `[an approval is waiting]`
  through the shell's printer, so the line is drawn visibly whatever the name holds, as any
  output is.

  A VM launched with no `notices` handle (a host, a VM that is no session) starts no listener. A
  send the steward could not deliver in its bound is lost: the steward is never held by a session
  that does not read. Only a send on the steward's badge counts: code in the session holds the
  receive right and may send through its own endpoint, but under badge 0, which the VM cannot
  mint away from, so a forged `[lease NAME ended]` prints nothing.
  """

  alias Redoubt.Shell.Printer
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
      # from: a forged notice prints nothing.
      {:request, nil, badge, _account, _labels, {words, buffer, handles}} ->
        print(badge, words, buffer || <<>>, handles)
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
  The line a notice sent on the steward's badge prints: `[lease NAME ended]` for `lease_ended`,
  `[an approval is waiting]` for `approval_waiting`; nothing for any other message, or for one on
  any other badge: a send through the endpoint from code in the session carries badge 0, so it
  cannot forge a notice.
  """
  @spec line(non_neg_integer(), [non_neg_integer()], binary(), list()) :: String.t() | nil
  def line(@notices, words, buffer, handles) do
    case P.decode(words, buffer, length(handles)) do
      {:ok, {:lease_ended, %{name: name}}} -> "[lease #{name} ended]"
      {:ok, {:approval_waiting, %{}}} -> "[an approval is waiting]"
      _other -> nil
    end
  end

  def line(_badge, _words, _buffer, _handles), do: nil

  defp print(badge, words, buffer, handles) do
    case line(badge, words, buffer, handles) do
      nil -> :ok
      text -> Printer.text(text)
    end
  end
end
