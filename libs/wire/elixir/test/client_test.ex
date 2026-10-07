# The generated clients (docs/servers/wire.md, "Generated clients") against a stand-in for
# beamlet's `call` and `send` natives, on the real BEAM: a call is encoded by the codec, its reply
# decoded with its handles by name, an error reply is the protocol's error by name and keeps none
# of the handles its reply brought (R13), a reply that does not decode is refused by its reason,
# and a request that does not encode makes no call. Run by libs/wire/elixir/run-vectors.

# The stand-in answers each call as the test scripts it, with the reply message beamlet's
# `call/3` sends; beamlet's own natives are the VM's, so this runs on the BEAM alone.
defmodule :redoubt do
  import Kernel, except: [send: 2]

  def call(conn, {words, buffer, handles}, timeout) do
    ref = make_ref()
    Kernel.send(self(), {:reply, ref, Process.get(:answer).({conn, words, buffer, handles, timeout})})
    {:ok, ref}
  end

  def send(conn, {words, buffer, handles}) do
    Kernel.send(self(), {:sent, conn, words, buffer, handles})
    :ok
  end
end

defmodule Redoubt.Wire.ClientTest do
  alias Redoubt.Wire.Client.{Keyd, NinepCommon}

  @doc "Runs every check, printing one line each, `client NAME: ok` or `FAIL`; returns `:ok` or `:failed`."
  def start do
    results = for {name, check} <- checks(), do: report(name, check)
    if Enum.all?(results), do: :ok, else: :failed
  end

  defp report(name, check) do
    ok =
      try do
        check.() == :ok
      rescue
        _ -> false
      end

    IO.puts("client #{name}: #{if ok, do: "ok", else: "FAIL"}")
    ok
  end

  defp answer(f), do: Process.put(:answer, f)

  defp checks do
    [
      {"a call is encoded and its reply decoded with its handles by name",
       fn ->
         answer(fn {:keyd, [5, 0, 0, 0], nil, [], 5000} -> {:ok, {[0, 7, 0, 0], nil, [:granted]}} end)
         same(Keyd.grant(:keyd), {:ok, %{id: 7, capability: :granted}})
       end},
      {"a buffer-shaped call lends its buffer and reads its reply from it",
       fn ->
         answer(fn {:keyd, [4, 8, 0, 0], <<4::little-32, "abcd">>, [], 100} ->
           {:ok, {[0, 4, 0, 0], <<1::little-32>>, []}}
         end)

         same(Keyd.holds(:keyd, "abcd", 100), {:ok, %{held: 1}})
       end},
      {"an error reply is the protocol's error by name",
       fn ->
         answer(fn _ -> {:ok, {[2, 0, 0, 0], nil, []}} end)
         :ok = same(Keyd.grant(:keyd), {:error, :not_permitted})
         answer(fn _ -> {:ok, {[3, 0, 0, 0], nil, []}} end)
         same(NinepCommon.new_connection(:fs, "/", 0), {:error, :refused})
       end},
      {"an error reply that carries a handle is refused and keeps none",
       fn ->
         answer(fn _ -> {:ok, {[2, 0, 0, 0], nil, [:stray]}} end)
         same(Keyd.grant(:keyd), {:error, :bad_handles})
       end},
      {"a reply that does not decode is refused by its reason and keeps none",
       fn ->
         answer(fn _ -> {:ok, {[0, 7, 0, 0], nil, [:one, :two]}} end)
         same(Keyd.grant(:keyd), {:error, :bad_handles})
       end},
      {"a call the platform did not make is its name",
       fn ->
         answer(fn _ -> {:error, :disconnected} end)
         same(Keyd.public_key(:keyd), {:error, :disconnected})
       end},
      {"a request that does not encode makes no call",
       fn ->
         answer(fn _ -> raise "no call is made" end)
         same(Keyd.release(:keyd, -1), {:error, :bad_value})
       end}
    ]
  end

  defp same(got, want) when got == want, do: :ok
  defp same(got, want), do: {:differs, got, want}
end
