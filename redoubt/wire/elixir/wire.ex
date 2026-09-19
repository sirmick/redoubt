# The shared part of the generated typed-message codecs (redoubt-wire's `typed` module, in
# Elixir). Hand-written; the per-protocol modules in proto/ are generated.
defmodule Redoubt.Wire do
  @moduledoc """
  Typed-message framing (planning/redoubt/WIRE.md), the Elixir twin of redoubt-wire's
  `typed` module; the two are held to the same test vectors.

  A message is four words (non-negative integers), a buffer (a binary, `<<>>` if none) and a
  handle count. Word 0 is the opcode. An **inline** message packs its fields into words
  1..3, four bytes per word, little-endian, zero-padded (12 bytes: the capacity of three
  32-bit words, so the layout is the same on rv32 and rv64). A **buffer** message puts its
  fields in the buffer and its length in word 1; words 2 and 3 are zero. Fields use 9P's
  encoding: little-endian integers, strings as a 16-bit length and UTF-8, byte arrays as a
  32-bit length and the bytes. Every word must fit in 32 bits.
  """

  import Bitwise

  @msize 65536
  @inline_bytes 12
  @max_u32 0xFFFFFFFF

  @doc "Runs an encoder, turning a thrown `{:wire, reason}` into `{:error, reason}`."
  def encoding(fun) do
    fun.()
  catch
    {:wire, reason} -> {:error, reason}
  end

  @doc "An unsigned integer of `bits` bits, little-endian; throws if out of range."
  def u(v, bits) when is_integer(v) and v >= 0 and v < 1 <<< bits, do: <<v::little-size(bits)>>
  def u(_, _), do: throw({:wire, :bad_value})

  @doc "A string: 16-bit length and UTF-8; throws if not UTF-8 or too long."
  def str(v) when is_binary(v) and byte_size(v) <= 0xFFFF do
    if String.valid?(v), do: [<<byte_size(v)::little-16>>, v], else: throw({:wire, :bad_value})
  end

  def str(_), do: throw({:wire, :bad_value})

  @doc "A byte array: 32-bit length and the bytes."
  def bytes(v) when is_binary(v) and byte_size(v) <= @max_u32, do: [<<byte_size(v)::little-32>>, v]
  def bytes(_), do: throw({:wire, :bad_value})

  @doc "An inline message's words (and no buffer)."
  def inline(opcode, iodata) do
    body = IO.iodata_to_binary(iodata)
    # The generator only makes a message inline if it fits; this guards against a bug there.
    if byte_size(body) > @inline_bytes, do: throw({:wire, :too_large})
    pad = @inline_bytes - byte_size(body)
    <<w1::little-32, w2::little-32, w3::little-32>> = <<body::binary, 0::size(pad * 8)>>
    {:ok, [opcode, w1, w2, w3], <<>>}
  end

  @doc "A buffer message's words and buffer."
  def buffer(opcode, iodata) do
    body = IO.iodata_to_binary(iodata)
    if byte_size(body) > @msize, do: throw({:wire, :too_large})
    {:ok, [opcode, byte_size(body), 0, 0], body}
  end

  @doc "The opcode of a received message; the words must be four integers, word 0 within 32 bits."
  def opcode([w0, w1, w2, w3])
      when is_integer(w0) and w0 >= 0 and w0 <= @max_u32 and is_integer(w1) and w1 >= 0 and
             is_integer(w2) and w2 >= 0 and is_integer(w3) and w3 >= 0,
      do: {:ok, w0}

  def opcode(_), do: {:error, :bad_words}

  @doc "The 12 inline bytes of a received message, which must have no buffer."
  def inline_body(_words, buffer) when buffer != <<>>, do: {:error, :unexpected_buffer}

  def inline_body([_, w1, w2, w3], _buffer)
      when w1 <= @max_u32 and w2 <= @max_u32 and w3 <= @max_u32,
      do: {:ok, <<w1::little-32, w2::little-32, w3::little-32>>}

  def inline_body(_, _), do: {:error, :bad_words}

  @doc "The encoded fields of a received buffer message: the first word-1 bytes of the buffer."
  def buffer_body([_, len, 0, 0], buffer) when len <= @msize do
    if len <= byte_size(buffer), do: {:ok, binary_part(buffer, 0, len)}, else: {:error, :short}
  end

  def buffer_body(_, _), do: {:error, :bad_words}

  @doc "Checks the handle count a message arrived with against its layout's."
  def check_handles(count, count), do: :ok
  def check_handles(_, _), do: {:error, :bad_handles}

  @doc "True if every byte is zero (an inline message's padding)."
  def zero?(bin), do: bin == :binary.copy(<<0>>, byte_size(bin))
end
