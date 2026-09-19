# A binary match that keeps the tail for a fallback and then reads on from the same position
# (bs_match's get_tail must not move the match position), as URI.decode does.
defmodule PercentTest do
  def start do
    for s <- ["%3Call+in%2F", "%3c%41", "a%zz", "%", "%4", "plain"] do
      {URI.decode(s), URI.decode_www_form(s), unpercent(s, "")}
    end
  end

  defp unpercent(<<?%, tail::binary>>, acc) do
    with <<h1, h2, rest::binary>> <- tail,
         d1 when is_integer(d1) <- hex(h1),
         d2 when is_integer(d2) <- hex(h2) do
      unpercent(rest, <<acc::binary, d1 * 16 + d2>>)
    else
      _ -> unpercent(tail, <<acc::binary, ?%>>)
    end
  end

  defp unpercent(<<c, tail::binary>>, acc), do: unpercent(tail, <<acc::binary, c>>)
  defp unpercent(<<>>, acc), do: acc

  defp hex(n) when n in ?0..?9, do: n - ?0
  defp hex(n) when n in ?A..?F, do: n - ?A + 10
  defp hex(n) when n in ?a..?f, do: n - ?a + 10
  defp hex(_), do: nil
end
