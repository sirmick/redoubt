defmodule Redoubt.Editor.Syntax.Erl do
  @moduledoc """
  Erlang, for the editor's highlighting (`Redoubt.Editor.Syntax`): `.erl` and `.hrl`. A quoted
  atom is a constant; a variable, which starts with a capital, is plain.
  """

  @behaviour Redoubt.Editor.Syntax

  alias Redoubt.Editor.Syntax

  @table Syntax.table(
           comment: ["%"],
           strings: [
             {~S|"|, ~S|"|, :string, true, true},
             {"'", "'", :constant, false, true}
           ],
           keywords: ~w(after and andalso band begin bnot bor bsl bsr bxor case catch cond div
             else end fun if let maybe not of or orelse receive rem try when xor),
           constants: ~w(true false undefined),
           char: "$"
         )

  @impl Syntax
  def line(text, state), do: Syntax.scan(@table, text, state)
end
