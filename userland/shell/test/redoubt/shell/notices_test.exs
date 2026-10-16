defmodule Redoubt.Shell.NoticesTest do
  use ExUnit.Case, async: true

  alias Redoubt.Shell.Notices
  alias Redoubt.Wire.Proto.Steward, as: P

  # The steward's send right on a session's notices endpoint carries badge 1 (servers/steward,
  # `NOTICES`); a send from code in the session, through the receive right it holds, carries
  # badge 0, the one badge the VM cannot mint away from.
  @steward 1
  @forged 0

  defp notice(name) do
    {:ok, words, buffer} = P.encode({:lease_ended, %{lease: 7, name: name}})
    {words, buffer}
  end

  defp restarted(server, slot) do
    {:ok, words, buffer} = P.encode({:server_restarted, %{server: server, slot: slot}})
    {words, buffer}
  end

  test "a lease_ended on the steward's badge prints the lease's name" do
    {words, buffer} = notice("alice/agent-1")
    assert Notices.line(@steward, words, buffer, []) == "[lease alice/agent-1 ended]"
    assert Notices.notice(@steward, words, buffer, []) == {:lease_ended, "alice/agent-1"}
  end

  test "an approval_waiting on the steward's badge prints that one is waiting, and nothing of it" do
    {:ok, words, buffer} = P.encode({:approval_waiting, %{}})
    assert Notices.line(@steward, words, buffer, []) == "[an approval is waiting]"
    assert Notices.line(@forged, words, buffer, []) == nil
  end

  test "a forged notice, sent through the session's own endpoint under badge 0, prints nothing" do
    {words, buffer} = notice("alice/agent-1")
    assert Notices.line(@forged, words, buffer, []) == nil
    assert Notices.line(2, words, buffer, []) == nil
    {words, buffer} = restarted("walfsd:data", 1)
    assert Notices.notice(@forged, words, buffer, []) == nil
    assert Notices.notice(2, words, buffer, []) == nil
  end

  test "any other message on the steward's badge prints nothing" do
    {:ok, words, buffer} = P.encode({:end_lease, %{lease: 7}})
    assert Notices.line(@steward, words, buffer, []) == nil
    assert Notices.notice(@steward, words, buffer, []) == nil
  end

  test "a server_restarted on the steward's badge names the server and the slot to re-bind" do
    {words, buffer} = restarted("walfsd:data", 1)
    assert Notices.notice(@steward, words, buffer, []) == {:server_restarted, "walfsd:data", 1}
    # Its line is the re-bind's, not the notice's.
    assert Notices.line(@steward, words, buffer, []) == nil
  end

  test "the restart's line says what the re-bind did" do
    assert Notices.restarted_line("walfsd:data", 1, {:ok, "/home/alice"}) ==
             "[walfsd:data restarted: /home/alice re-bound]"

    assert Notices.restarted_line("walfsd:data", 1, {:error, :unknown}) ==
             "[walfsd:data restarted: slot 1 not re-bound (:unknown)]"
  end
end
