# The shared part of the generated clients (docs/servers/wire.md, "Generated clients").
# Hand-written; the per-protocol modules in ../client/ are generated and call these.
defmodule Redoubt.Wire.Client do
  @moduledoc """
  One typed call over beamlet's natives (docs/userland/beamlet.md, "Natives"): encode the request
  with the protocol's codec, make the call with `:redoubt.call/3`, wait for its reply message, and
  decode it. Nothing here holds authority: the connection is the caller's, the check is the
  server's.

  - A reply is `{:ok, reply}`: a map of its fields and its handles by the table's names, each
    handle a resource the caller now holds.
  - An error is `{:error, name}`: the protocol's error code by its table's name, a codec error
    (`:bad_message`, `:short_fields`, ...), or why the platform did not make the call (`:busy`,
    `:timeout`, `:disconnected`, ...). An error reply, or one that does not decode, keeps none of
    the handles it brought: they are dropped here and closed when collected (R13).
  - The platform answers every call it took, by its timeout once a thread takes it; the wait here
    is bounded too, by the timeout and `@queued`, for a call that waits for a thread first. A reply
    that comes later stays in the caller's mailbox, with its handles, until the caller drops it.
  - Every generated function takes its timeout last, `timeout/0` by default: the bound for a
    short call. A call measured past it passes its own: a lease's launch, `launch_timeout/0`.
  """

  # beamlet's natives (docs/userland/beamlet.md, "Natives"): no module, so nothing to check at compile time.
  @compile {:no_warn_undefined, :redoubt}

  @timeout 5000
  # How long a call may wait for one of the platform's call threads, beyond its own timeout (ms).
  @queued 5000
  # The steward's `start_agent` is the one call measured past `@timeout`: its served time, the
  # steward's batch (the lease's carve and mints at the shared servers, the VM's reads of its
  # program) beside the sponsor's other leases spinning, is at worst 6.64 s under
  # `steward-latency`'s rounds on both widths at one and two harts (rv32, two harts, the N = 16
  # round's launch beside fourteen spinners; rv32 at one hart 5.851 s beside three, rv64 at
  # worst 3.95 s); twice that, 13.28 s, rounded up to a second (docs/servers/steward.md,
  # "Residual risks"; docs/kernel/scheduling.md, "Responsiveness").
  @launch_timeout 14_000

  @doc "The longest a call waits, in milliseconds: the natives' bound."
  def timeout, do: @timeout

  @doc "The longest a lease's launch (`start_agent`) waits, in milliseconds: twice the worst launch measured under load."
  def launch_timeout, do: @launch_timeout

  @doc "Calls `conn` with `message`, a request of `proto`, carrying `handles`."
  def call(conn, proto, {name, _} = message, handles, timeout) do
    case proto.layout(name) do
      {opcode, shape, _fields, _handles, {_reply_fields, reply_handles}} ->
        with {:ok, words, buffer} <- proto.encode(message),
             {:ok, ref} <- :redoubt.call(conn, {words, lent(shape, buffer), handles}, timeout) do
          receive do
            {:reply, ^ref, {:ok, {rwords, rbuffer, rhandles}}} ->
              decode(proto, opcode, rwords, rbuffer || <<>>, rhandles, reply_handles)

            {:reply, ^ref, {:error, _} = error} ->
              error
          after
            timeout + @queued -> {:error, :timeout}
          end
        end

      nil ->
        {:error, :bad_message}
    end
  end

  @doc "Sends `message`, a request of `proto`, on `conn`, carrying `handles`: one way."
  def send(conn, proto, {name, _} = message, handles) do
    case proto.layout(name) do
      {_opcode, shape, _fields, _handles, _reply} ->
        with {:ok, words, buffer} <- proto.encode(message) do
          :redoubt.send(conn, {words, lent(shape, buffer), handles})
        end

      nil ->
        {:error, :bad_message}
    end
  end

  # An inline message lends nothing; a buffer-shaped one lends its buffer, which the reply comes
  # back in.
  defp lent(:inline, _buffer), do: nil
  defp lent(:buffer, buffer), do: buffer

  defp decode(proto, opcode, words, buffer, handles, names) do
    case proto.decode_reply(opcode, words, buffer, length(handles)) do
      {:ok, {_name, fields}} -> {:ok, Map.merge(fields, Map.new(Enum.zip(names, handles)))}
      {:failed, error} -> {:error, error}
      {:error, _} = error -> error
    end
  end
end
