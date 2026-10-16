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

  test "a lease_ended on the steward's badge prints the lease's name" do
    {words, buffer} = notice("alice/agent-1")
    assert Notices.line(@steward, words, buffer, []) == "[lease alice/agent-1 ended]"
  end

  test "a forged notice, sent through the session's own endpoint under badge 0, prints nothing" do
    {words, buffer} = notice("alice/agent-1")
    assert Notices.line(@forged, words, buffer, []) == nil
    assert Notices.line(2, words, buffer, []) == nil
  end

  test "any other message on the steward's badge prints nothing" do
    {:ok, words, buffer} = P.encode({:end_lease, %{lease: 7}})
    assert Notices.line(@steward, words, buffer, []) == nil
  end
end
