var nbtEditor = (function (exports) {
    'use strict';

    function hasGzipHeader(array) {
        var head = array.slice(0, 2);
        return head.length === 2 && head[0] === 0x1f && head[1] === 0x8b;
    }
    function hasZlibHeader(array) {
        const head = array.slice(0, 2);
        return head.length === 2 && head[0] === 0x78
            && (head[1] === 0x01 || head[1] === 0x5e || head[1] === 0x9c || head[2] === 0xda);
    }
    function getBedrockHeader(array) {
        const head = array.slice(0, 8);
        const view = new DataView(head.buffer, head.byteOffset);
        const version = view.getUint32(0, true);
        const length = view.getUint32(4, true);
        if (head.length === 8 && version > 0 && version < 100 && length === array.byteLength - 8) {
            return version;
        }
        return undefined;
    }
    function encodeUTF8(str) {
        var array = [], i, c;
        for (i = 0; i < str.length; i++) {
            c = str.charCodeAt(i);
            if (c < 0x80) {
                array.push(c);
            }
            else if (c < 0x800) {
                array.push(0xC0 | c >> 6);
                array.push(0x80 | c & 0x3F);
            }
            else if (c < 0x10000) {
                array.push(0xE0 | c >> 12);
                array.push(0x80 | (c >> 6) & 0x3F);
                array.push(0x80 | c & 0x3F);
            }
            else {
                array.push(0xF0 | (c >> 18) & 0x07);
                array.push(0x80 | (c >> 12) & 0x3F);
                array.push(0x80 | (c >> 6) & 0x3F);
                array.push(0x80 | c & 0x3F);
            }
        }
        return array;
    }
    function decodeUTF8(array) {
        var codepoints = [], i;
        for (i = 0; i < array.length; i++) {
            if ((array[i] & 0x80) === 0) {
                codepoints.push(array[i] & 0x7F);
            }
            else if (i + 1 < array.length &&
                (array[i] & 0xE0) === 0xC0 &&
                (array[i + 1] & 0xC0) === 0x80) {
                codepoints.push(((array[i] & 0x1F) << 6) |
                    (array[i + 1] & 0x3F));
            }
            else if (i + 2 < array.length &&
                (array[i] & 0xF0) === 0xE0 &&
                (array[i + 1] & 0xC0) === 0x80 &&
                (array[i + 2] & 0xC0) === 0x80) {
                codepoints.push(((array[i] & 0x0F) << 12) |
                    ((array[i + 1] & 0x3F) << 6) |
                    (array[i + 2] & 0x3F));
            }
            else if (i + 3 < array.length &&
                (array[i] & 0xF8) === 0xF0 &&
                (array[i + 1] & 0xC0) === 0x80 &&
                (array[i + 2] & 0xC0) === 0x80 &&
                (array[i + 3] & 0xC0) === 0x80) {
                codepoints.push(((array[i] & 0x07) << 18) |
                    ((array[i + 1] & 0x3F) << 12) |
                    ((array[i + 2] & 0x3F) << 6) |
                    (array[i + 3] & 0x3F));
            }
        }
        return String.fromCharCode.apply(null, codepoints);
    }

    class RawDataInput {
        littleEndian;
        offset;
        array;
        view;
        constructor(input, options) {
            this.littleEndian = options?.littleEndian ?? false;
            this.offset = options?.offset ?? 0;
            this.array = input instanceof Uint8Array ? input : new Uint8Array(input);
            this.view = new DataView(this.array.buffer, this.array.byteOffset);
        }
        readNumber(type, size) {
            const value = this.view[type](this.offset, this.littleEndian);
            this.offset += size;
            return value;
        }
        readByte = this.readNumber.bind(this, 'getInt8', 1);
        readShort = this.readNumber.bind(this, 'getInt16', 2);
        readInt = this.readNumber.bind(this, 'getInt32', 4);
        readFloat = this.readNumber.bind(this, 'getFloat32', 4);
        readDouble = this.readNumber.bind(this, 'getFloat64', 8);
        readBytes(length) {
            const bytes = this.array.slice(this.offset, this.offset + length);
            this.offset += length;
            return bytes;
        }
        readString() {
            const length = this.readShort();
            const bytes = this.readBytes(length);
            return decodeUTF8(bytes);
        }
    }

    class RawDataOutput {
        littleEndian;
        offset;
        buffer;
        array;
        view;
        constructor(options) {
            this.littleEndian = options?.littleEndian ?? false;
            this.offset = options?.offset ?? 0;
            this.buffer = new ArrayBuffer(options?.initialSize ?? 1024);
            this.array = new Uint8Array(this.buffer);
            this.view = new DataView(this.buffer);
        }
        accommodate(size) {
            const requiredLength = this.offset + size;
            if (this.buffer.byteLength >= requiredLength) {
                return;
            }
            let newLength = this.buffer.byteLength;
            while (newLength < requiredLength) {
                newLength *= 2;
            }
            const newBuffer = new ArrayBuffer(newLength);
            const newArray = new Uint8Array(newBuffer);
            newArray.set(this.array);
            if (this.offset > this.buffer.byteLength) {
                newArray.fill(0, this.buffer.byteLength, this.offset);
            }
            this.buffer = newBuffer;
            this.view = new DataView(newBuffer);
            this.array = newArray;
        }
        writeNumber(type, size, value) {
            this.accommodate(size);
            this.view[type](this.offset, value, this.littleEndian);
            this.offset += size;
        }
        writeByte = this.writeNumber.bind(this, 'setInt8', 1);
        writeShort = this.writeNumber.bind(this, 'setInt16', 2);
        writeInt = this.writeNumber.bind(this, 'setInt32', 4);
        writeFloat = this.writeNumber.bind(this, 'setFloat32', 4);
        writeDouble = this.writeNumber.bind(this, 'setFloat64', 8);
        writeBytes(bytes) {
            this.accommodate(bytes.length);
            this.array.set(bytes, this.offset);
            this.offset += bytes.length;
        }
        writeString(value) {
            const bytes = encodeUTF8(value);
            this.writeShort(bytes.length);
            this.writeBytes(bytes);
        }
        getData() {
            this.accommodate(0);
            return this.array.slice(0, this.offset);
        }
    }

    var Json;
    (function (Json) {
        function readNumber(obj) {
            return typeof obj === 'number' ? obj : undefined;
        }
        Json.readNumber = readNumber;
        function readInt(obj) {
            return typeof obj === 'number' ? Math.floor(obj) : undefined;
        }
        Json.readInt = readInt;
        function readString(obj) {
            return typeof obj === 'string' ? obj : undefined;
        }
        Json.readString = readString;
        function readBoolean(obj) {
            return typeof obj === 'boolean' ? obj : undefined;
        }
        Json.readBoolean = readBoolean;
        function readObject(obj) {
            return typeof obj === 'object' && obj !== null && !Array.isArray(obj)
                ? obj
                : undefined;
        }
        Json.readObject = readObject;
        function readArray(obj, parser) {
            if (!Array.isArray(obj))
                return undefined;
            if (!parser)
                return obj;
            return obj.map(el => parser(el));
        }
        Json.readArray = readArray;
        function readPair(obj, parser) {
            if (!Array.isArray(obj))
                return undefined;
            return [0, 1].map((i => parser(obj[i])));
        }
        Json.readPair = readPair;
        function readMap(obj, parser) {
            const root = readObject(obj) ?? {};
            return Object.fromEntries(Object.entries(root).map(([k, v]) => [k, parser(v)]));
        }
        Json.readMap = readMap;
        function compose(obj, parser, mapper) {
            const result = parser(obj);
            return result ? mapper(result) : undefined;
        }
        Json.compose = compose;
        function readEnum(obj, values) {
            if (typeof obj !== 'string')
                return values[0];
            if (values.includes(obj))
                return obj;
            return values[0];
        }
        Json.readEnum = readEnum;
    })(Json || (Json = {}));

    var Color;
    (function (Color) {
        function fromJson(obj) {
            const packed = Json.readNumber(obj);
            if (packed)
                return intToRgb(packed);
            const array = Json.readArray(obj, o => Json.readNumber(o) ?? 0);
            if (array === undefined || array.length !== 3)
                return undefined;
            return array;
        }
        Color.fromJson = fromJson;
        function fromNbt(nbt) {
            if (nbt.isNumber())
                return intToRgb(nbt.getAsNumber());
            if (!nbt.isListOrArray())
                return undefined;
            const values = nbt.getItems();
            if (values.length < 3)
                return undefined;
            return values.map(i => i.getAsNumber());
        }
        Color.fromNbt = fromNbt;
        function intToRgb(n) {
            const r = (n >> 16) & 255;
            const g = (n >> 8) & 255;
            const b = n & 255;
            return [r / 255, g / 255, b / 255];
        }
        Color.intToRgb = intToRgb;
    })(Color || (Color = {}));

    class StringReader {
        source;
        cursor;
        constructor(source) {
            this.source = source;
            this.cursor = 0;
        }
        get remainingLength() {
            return this.source.length - this.cursor;
        }
        get totalLength() {
            return this.source.length;
        }
        getRead(start = 0) {
            return this.source.substring(start, this.cursor);
        }
        getRemaining() {
            return this.source.substring(this.cursor);
        }
        canRead(length = 1) {
            return this.cursor + length <= this.source.length;
        }
        peek(offset = 0) {
            return this.source.charAt(this.cursor + offset);
        }
        read() {
            return this.source.charAt(this.cursor++);
        }
        skip() {
            this.cursor += 1;
        }
        skipWhitespace() {
            while (this.canRead() && StringReader.isWhitespace(this.peek())) {
                this.skip();
            }
        }
        expect(c, skipWhitespace = false) {
            if (skipWhitespace) {
                this.skipWhitespace();
            }
            if (!this.canRead() || this.peek() !== c) {
                throw this.createError(`Expected '${c}'`);
            }
            this.skip();
        }
        readInt() {
            const start = this.cursor;
            while (this.canRead() && StringReader.isAllowedInNumber(this.peek())) {
                this.skip();
            }
            const number = this.getRead(start);
            if (number.length === 0) {
                throw this.createError('Expected integer');
            }
            try {
                const value = Number(number);
                if (isNaN(value) || !Number.isInteger(value)) {
                    throw new Error();
                }
                return value;
            }
            catch (e) {
                this.cursor = start;
                throw this.createError(`Invalid integer '${number}'`);
            }
        }
        readFloat() {
            const start = this.cursor;
            while (this.canRead() && StringReader.isAllowedInNumber(this.peek())) {
                this.skip();
            }
            const number = this.getRead(start);
            if (number.length === 0) {
                throw this.createError('Expected float');
            }
            try {
                const value = Number(number);
                if (isNaN(value)) {
                    throw new Error();
                }
                return value;
            }
            catch (e) {
                this.cursor = start;
                throw this.createError(`Invalid float '${number}'`);
            }
        }
        readUnquotedString() {
            const start = this.cursor;
            while (this.canRead() && StringReader.isAllowedInUnquotedString(this.peek())) {
                this.skip();
            }
            return this.getRead(start);
        }
        readQuotedString() {
            if (!this.canRead()) {
                return '';
            }
            const c = this.peek();
            if (!StringReader.isQuotedStringStart(c)) {
                throw this.createError('Expected quote to start a string');
            }
            this.skip();
            return this.readStringUntil(c);
        }
        readString() {
            if (!this.canRead()) {
                return '';
            }
            const c = this.peek();
            if (StringReader.isQuotedStringStart(c)) {
                this.skip();
                return this.readStringUntil(c);
            }
            return this.readUnquotedString();
        }
        readStringUntil(terminator) {
            const result = [];
            let escaped = false;
            while (this.canRead()) {
                const c = this.read();
                if (escaped) {
                    if (c === terminator || c === '\\') {
                        result.push(c);
                        escaped = false;
                    }
                    else {
                        this.cursor -= 1;
                        throw this.createError(`Invalid escape sequence '${c}' in quoted string`);
                    }
                }
                else if (c === '\\') {
                    escaped = true;
                }
                else if (c === terminator) {
                    return result.join('');
                }
                else {
                    result.push(c);
                }
            }
            throw this.createError('Unclosed quoted string');
        }
        readBoolean() {
            const start = this.cursor;
            const value = this.readUnquotedString();
            if (value.length === 0) {
                throw this.createError('Expected bool');
            }
            if (value === 'true') {
                return true;
            }
            else if (value === 'false') {
                return false;
            }
            else {
                this.cursor = start;
                throw this.createError(`Invalid bool, expected true or false but found '${value}'`);
            }
        }
        static isAllowedInNumber(c) {
            return (c >= '0' && c <= '9') || c === '.' || c === '-';
        }
        static isAllowedInUnquotedString(c) {
            return (c >= '0' && c <= '9')
                || (c >= 'A' && c <= 'Z')
                || (c >= 'a' && c <= 'z')
                || c === '_'
                || c === '-'
                || c === '.'
                || c === '+';
        }
        static isQuotedStringStart(c) {
            return c === "'" || c === '"';
        }
        static isWhitespace(c) {
            return c === ' ' || c === '\t' || c === '\n' || c === '\r';
        }
        createError(message) {
            const cursor = Math.min(this.source.length, this.cursor);
            const context = (cursor > 10 ? '...' : '') + this.source.substring(Math.max(0, cursor - 10), cursor);
            return new Error(`${message} at position ${this.cursor}: ${context}<--[HERE]`);
        }
    }

    function lazy$1(getter) {
        let value = null;
        return () => {
            if (value == null) {
                value = getter();
            }
            return value;
        };
    }
    function computeIfAbsent(map, key, getter) {
        const existing = map.get(key);
        if (existing !== undefined) {
            return existing;
        }
        const value = getter(key);
        map.set(key, value);
        return value;
    }

    /*! pako 2.1.0 https://github.com/nodeca/pako @license (MIT AND Zlib) */
    // (C) 1995-2013 Jean-loup Gailly and Mark Adler
    // (C) 2014-2017 Vitaly Puzrin and Andrey Tupitsin
    //
    // This software is provided 'as-is', without any express or implied
    // warranty. In no event will the authors be held liable for any damages
    // arising from the use of this software.
    //
    // Permission is granted to anyone to use this software for any purpose,
    // including commercial applications, and to alter it and redistribute it
    // freely, subject to the following restrictions:
    //
    // 1. The origin of this software must not be misrepresented; you must not
    //   claim that you wrote the original software. If you use this software
    //   in a product, an acknowledgment in the product documentation would be
    //   appreciated but is not required.
    // 2. Altered source versions must be plainly marked as such, and must not be
    //   misrepresented as being the original software.
    // 3. This notice may not be removed or altered from any source distribution.

    /* eslint-disable space-unary-ops */

    /* Public constants ==========================================================*/
    /* ===========================================================================*/


    //const Z_FILTERED          = 1;
    //const Z_HUFFMAN_ONLY      = 2;
    //const Z_RLE               = 3;
    const Z_FIXED$1               = 4;
    //const Z_DEFAULT_STRATEGY  = 0;

    /* Possible values of the data_type field (though see inflate()) */
    const Z_BINARY              = 0;
    const Z_TEXT                = 1;
    //const Z_ASCII             = 1; // = Z_TEXT
    const Z_UNKNOWN$1             = 2;

    /*============================================================================*/


    function zero$1(buf) { let len = buf.length; while (--len >= 0) { buf[len] = 0; } }

    // From zutil.h

    const STORED_BLOCK = 0;
    const STATIC_TREES = 1;
    const DYN_TREES    = 2;
    /* The three kinds of block type */

    const MIN_MATCH$1    = 3;
    const MAX_MATCH$1    = 258;
    /* The minimum and maximum match lengths */

    // From deflate.h
    /* ===========================================================================
     * Internal compression state.
     */

    const LENGTH_CODES$1  = 29;
    /* number of length codes, not counting the special END_BLOCK code */

    const LITERALS$1      = 256;
    /* number of literal bytes 0..255 */

    const L_CODES$1       = LITERALS$1 + 1 + LENGTH_CODES$1;
    /* number of Literal or Length codes, including the END_BLOCK code */

    const D_CODES$1       = 30;
    /* number of distance codes */

    const BL_CODES$1      = 19;
    /* number of codes used to transfer the bit lengths */

    const HEAP_SIZE$1     = 2 * L_CODES$1 + 1;
    /* maximum heap size */

    const MAX_BITS$1      = 15;
    /* All codes must not exceed MAX_BITS bits */

    const Buf_size      = 16;
    /* size of bit buffer in bi_buf */


    /* ===========================================================================
     * Constants
     */

    const MAX_BL_BITS = 7;
    /* Bit length codes must not exceed MAX_BL_BITS bits */

    const END_BLOCK   = 256;
    /* end of block literal code */

    const REP_3_6     = 16;
    /* repeat previous bit length 3-6 times (2 bits of repeat count) */

    const REPZ_3_10   = 17;
    /* repeat a zero length 3-10 times  (3 bits of repeat count) */

    const REPZ_11_138 = 18;
    /* repeat a zero length 11-138 times  (7 bits of repeat count) */

    /* eslint-disable comma-spacing,array-bracket-spacing */
    const extra_lbits =   /* extra bits for each length code */
      new Uint8Array([0,0,0,0,0,0,0,0,1,1,1,1,2,2,2,2,3,3,3,3,4,4,4,4,5,5,5,5,0]);

    const extra_dbits =   /* extra bits for each distance code */
      new Uint8Array([0,0,0,0,1,1,2,2,3,3,4,4,5,5,6,6,7,7,8,8,9,9,10,10,11,11,12,12,13,13]);

    const extra_blbits =  /* extra bits for each bit length code */
      new Uint8Array([0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,2,3,7]);

    const bl_order =
      new Uint8Array([16,17,18,0,8,7,9,6,10,5,11,4,12,3,13,2,14,1,15]);
    /* eslint-enable comma-spacing,array-bracket-spacing */

    /* The lengths of the bit length codes are sent in order of decreasing
     * probability, to avoid transmitting the lengths for unused bit length codes.
     */

    /* ===========================================================================
     * Local data. These are initialized only once.
     */

    // We pre-fill arrays with 0 to avoid uninitialized gaps

    const DIST_CODE_LEN = 512; /* see definition of array dist_code below */

    // !!!! Use flat array instead of structure, Freq = i*2, Len = i*2+1
    const static_ltree  = new Array((L_CODES$1 + 2) * 2);
    zero$1(static_ltree);
    /* The static literal tree. Since the bit lengths are imposed, there is no
     * need for the L_CODES extra codes used during heap construction. However
     * The codes 286 and 287 are needed to build a canonical tree (see _tr_init
     * below).
     */

    const static_dtree  = new Array(D_CODES$1 * 2);
    zero$1(static_dtree);
    /* The static distance tree. (Actually a trivial tree since all codes use
     * 5 bits.)
     */

    const _dist_code    = new Array(DIST_CODE_LEN);
    zero$1(_dist_code);
    /* Distance codes. The first 256 values correspond to the distances
     * 3 .. 258, the last 256 values correspond to the top 8 bits of
     * the 15 bit distances.
     */

    const _length_code  = new Array(MAX_MATCH$1 - MIN_MATCH$1 + 1);
    zero$1(_length_code);
    /* length code for each normalized match length (0 == MIN_MATCH) */

    const base_length   = new Array(LENGTH_CODES$1);
    zero$1(base_length);
    /* First normalized length for each code (0 = MIN_MATCH) */

    const base_dist     = new Array(D_CODES$1);
    zero$1(base_dist);
    /* First normalized distance for each code (0 = distance of 1) */


    function StaticTreeDesc(static_tree, extra_bits, extra_base, elems, max_length) {

      this.static_tree  = static_tree;  /* static tree or NULL */
      this.extra_bits   = extra_bits;   /* extra bits for each code or NULL */
      this.extra_base   = extra_base;   /* base index for extra_bits */
      this.elems        = elems;        /* max number of elements in the tree */
      this.max_length   = max_length;   /* max bit length for the codes */

      // show if `static_tree` has data or dummy - needed for monomorphic objects
      this.has_stree    = static_tree && static_tree.length;
    }


    let static_l_desc;
    let static_d_desc;
    let static_bl_desc;


    function TreeDesc(dyn_tree, stat_desc) {
      this.dyn_tree = dyn_tree;     /* the dynamic tree */
      this.max_code = 0;            /* largest code with non zero frequency */
      this.stat_desc = stat_desc;   /* the corresponding static tree */
    }



    const d_code = (dist) => {

      return dist < 256 ? _dist_code[dist] : _dist_code[256 + (dist >>> 7)];
    };


    /* ===========================================================================
     * Output a short LSB first on the stream.
     * IN assertion: there is enough room in pendingBuf.
     */
    const put_short = (s, w) => {
    //    put_byte(s, (uch)((w) & 0xff));
    //    put_byte(s, (uch)((ush)(w) >> 8));
      s.pending_buf[s.pending++] = (w) & 0xff;
      s.pending_buf[s.pending++] = (w >>> 8) & 0xff;
    };


    /* ===========================================================================
     * Send a value on a given number of bits.
     * IN assertion: length <= 16 and value fits in length bits.
     */
    const send_bits = (s, value, length) => {

      if (s.bi_valid > (Buf_size - length)) {
        s.bi_buf |= (value << s.bi_valid) & 0xffff;
        put_short(s, s.bi_buf);
        s.bi_buf = value >> (Buf_size - s.bi_valid);
        s.bi_valid += length - Buf_size;
      } else {
        s.bi_buf |= (value << s.bi_valid) & 0xffff;
        s.bi_valid += length;
      }
    };


    const send_code = (s, c, tree) => {

      send_bits(s, tree[c * 2]/*.Code*/, tree[c * 2 + 1]/*.Len*/);
    };


    /* ===========================================================================
     * Reverse the first len bits of a code, using straightforward code (a faster
     * method would use a table)
     * IN assertion: 1 <= len <= 15
     */
    const bi_reverse = (code, len) => {

      let res = 0;
      do {
        res |= code & 1;
        code >>>= 1;
        res <<= 1;
      } while (--len > 0);
      return res >>> 1;
    };


    /* ===========================================================================
     * Flush the bit buffer, keeping at most 7 bits in it.
     */
    const bi_flush = (s) => {

      if (s.bi_valid === 16) {
        put_short(s, s.bi_buf);
        s.bi_buf = 0;
        s.bi_valid = 0;

      } else if (s.bi_valid >= 8) {
        s.pending_buf[s.pending++] = s.bi_buf & 0xff;
        s.bi_buf >>= 8;
        s.bi_valid -= 8;
      }
    };


    /* ===========================================================================
     * Compute the optimal bit lengths for a tree and update the total bit length
     * for the current block.
     * IN assertion: the fields freq and dad are set, heap[heap_max] and
     *    above are the tree nodes sorted by increasing frequency.
     * OUT assertions: the field len is set to the optimal bit length, the
     *     array bl_count contains the frequencies for each bit length.
     *     The length opt_len is updated; static_len is also updated if stree is
     *     not null.
     */
    const gen_bitlen = (s, desc) => {
    //    deflate_state *s;
    //    tree_desc *desc;    /* the tree descriptor */

      const tree            = desc.dyn_tree;
      const max_code        = desc.max_code;
      const stree           = desc.stat_desc.static_tree;
      const has_stree       = desc.stat_desc.has_stree;
      const extra           = desc.stat_desc.extra_bits;
      const base            = desc.stat_desc.extra_base;
      const max_length      = desc.stat_desc.max_length;
      let h;              /* heap index */
      let n, m;           /* iterate over the tree elements */
      let bits;           /* bit length */
      let xbits;          /* extra bits */
      let f;              /* frequency */
      let overflow = 0;   /* number of elements with bit length too large */

      for (bits = 0; bits <= MAX_BITS$1; bits++) {
        s.bl_count[bits] = 0;
      }

      /* In a first pass, compute the optimal bit lengths (which may
       * overflow in the case of the bit length tree).
       */
      tree[s.heap[s.heap_max] * 2 + 1]/*.Len*/ = 0; /* root of the heap */

      for (h = s.heap_max + 1; h < HEAP_SIZE$1; h++) {
        n = s.heap[h];
        bits = tree[tree[n * 2 + 1]/*.Dad*/ * 2 + 1]/*.Len*/ + 1;
        if (bits > max_length) {
          bits = max_length;
          overflow++;
        }
        tree[n * 2 + 1]/*.Len*/ = bits;
        /* We overwrite tree[n].Dad which is no longer needed */

        if (n > max_code) { continue; } /* not a leaf node */

        s.bl_count[bits]++;
        xbits = 0;
        if (n >= base) {
          xbits = extra[n - base];
        }
        f = tree[n * 2]/*.Freq*/;
        s.opt_len += f * (bits + xbits);
        if (has_stree) {
          s.static_len += f * (stree[n * 2 + 1]/*.Len*/ + xbits);
        }
      }
      if (overflow === 0) { return; }

      // Tracev((stderr,"\nbit length overflow\n"));
      /* This happens for example on obj2 and pic of the Calgary corpus */

      /* Find the first bit length which could increase: */
      do {
        bits = max_length - 1;
        while (s.bl_count[bits] === 0) { bits--; }
        s.bl_count[bits]--;      /* move one leaf down the tree */
        s.bl_count[bits + 1] += 2; /* move one overflow item as its brother */
        s.bl_count[max_length]--;
        /* The brother of the overflow item also moves one step up,
         * but this does not affect bl_count[max_length]
         */
        overflow -= 2;
      } while (overflow > 0);

      /* Now recompute all bit lengths, scanning in increasing frequency.
       * h is still equal to HEAP_SIZE. (It is simpler to reconstruct all
       * lengths instead of fixing only the wrong ones. This idea is taken
       * from 'ar' written by Haruhiko Okumura.)
       */
      for (bits = max_length; bits !== 0; bits--) {
        n = s.bl_count[bits];
        while (n !== 0) {
          m = s.heap[--h];
          if (m > max_code) { continue; }
          if (tree[m * 2 + 1]/*.Len*/ !== bits) {
            // Tracev((stderr,"code %d bits %d->%d\n", m, tree[m].Len, bits));
            s.opt_len += (bits - tree[m * 2 + 1]/*.Len*/) * tree[m * 2]/*.Freq*/;
            tree[m * 2 + 1]/*.Len*/ = bits;
          }
          n--;
        }
      }
    };


    /* ===========================================================================
     * Generate the codes for a given tree and bit counts (which need not be
     * optimal).
     * IN assertion: the array bl_count contains the bit length statistics for
     * the given tree and the field len is set for all tree elements.
     * OUT assertion: the field code is set for all tree elements of non
     *     zero code length.
     */
    const gen_codes = (tree, max_code, bl_count) => {
    //    ct_data *tree;             /* the tree to decorate */
    //    int max_code;              /* largest code with non zero frequency */
    //    ushf *bl_count;            /* number of codes at each bit length */

      const next_code = new Array(MAX_BITS$1 + 1); /* next code value for each bit length */
      let code = 0;              /* running code value */
      let bits;                  /* bit index */
      let n;                     /* code index */

      /* The distribution counts are first used to generate the code values
       * without bit reversal.
       */
      for (bits = 1; bits <= MAX_BITS$1; bits++) {
        code = (code + bl_count[bits - 1]) << 1;
        next_code[bits] = code;
      }
      /* Check that the bit counts in bl_count are consistent. The last code
       * must be all ones.
       */
      //Assert (code + bl_count[MAX_BITS]-1 == (1<<MAX_BITS)-1,
      //        "inconsistent bit counts");
      //Tracev((stderr,"\ngen_codes: max_code %d ", max_code));

      for (n = 0;  n <= max_code; n++) {
        let len = tree[n * 2 + 1]/*.Len*/;
        if (len === 0) { continue; }
        /* Now reverse the bits */
        tree[n * 2]/*.Code*/ = bi_reverse(next_code[len]++, len);

        //Tracecv(tree != static_ltree, (stderr,"\nn %3d %c l %2d c %4x (%x) ",
        //     n, (isgraph(n) ? n : ' '), len, tree[n].Code, next_code[len]-1));
      }
    };


    /* ===========================================================================
     * Initialize the various 'constant' tables.
     */
    const tr_static_init = () => {

      let n;        /* iterates over tree elements */
      let bits;     /* bit counter */
      let length;   /* length value */
      let code;     /* code value */
      let dist;     /* distance index */
      const bl_count = new Array(MAX_BITS$1 + 1);
      /* number of codes at each bit length for an optimal tree */

      // do check in _tr_init()
      //if (static_init_done) return;

      /* For some embedded targets, global variables are not initialized: */
    /*#ifdef NO_INIT_GLOBAL_POINTERS
      static_l_desc.static_tree = static_ltree;
      static_l_desc.extra_bits = extra_lbits;
      static_d_desc.static_tree = static_dtree;
      static_d_desc.extra_bits = extra_dbits;
      static_bl_desc.extra_bits = extra_blbits;
    #endif*/

      /* Initialize the mapping length (0..255) -> length code (0..28) */
      length = 0;
      for (code = 0; code < LENGTH_CODES$1 - 1; code++) {
        base_length[code] = length;
        for (n = 0; n < (1 << extra_lbits[code]); n++) {
          _length_code[length++] = code;
        }
      }
      //Assert (length == 256, "tr_static_init: length != 256");
      /* Note that the length 255 (match length 258) can be represented
       * in two different ways: code 284 + 5 bits or code 285, so we
       * overwrite length_code[255] to use the best encoding:
       */
      _length_code[length - 1] = code;

      /* Initialize the mapping dist (0..32K) -> dist code (0..29) */
      dist = 0;
      for (code = 0; code < 16; code++) {
        base_dist[code] = dist;
        for (n = 0; n < (1 << extra_dbits[code]); n++) {
          _dist_code[dist++] = code;
        }
      }
      //Assert (dist == 256, "tr_static_init: dist != 256");
      dist >>= 7; /* from now on, all distances are divided by 128 */
      for (; code < D_CODES$1; code++) {
        base_dist[code] = dist << 7;
        for (n = 0; n < (1 << (extra_dbits[code] - 7)); n++) {
          _dist_code[256 + dist++] = code;
        }
      }
      //Assert (dist == 256, "tr_static_init: 256+dist != 512");

      /* Construct the codes of the static literal tree */
      for (bits = 0; bits <= MAX_BITS$1; bits++) {
        bl_count[bits] = 0;
      }

      n = 0;
      while (n <= 143) {
        static_ltree[n * 2 + 1]/*.Len*/ = 8;
        n++;
        bl_count[8]++;
      }
      while (n <= 255) {
        static_ltree[n * 2 + 1]/*.Len*/ = 9;
        n++;
        bl_count[9]++;
      }
      while (n <= 279) {
        static_ltree[n * 2 + 1]/*.Len*/ = 7;
        n++;
        bl_count[7]++;
      }
      while (n <= 287) {
        static_ltree[n * 2 + 1]/*.Len*/ = 8;
        n++;
        bl_count[8]++;
      }
      /* Codes 286 and 287 do not exist, but we must include them in the
       * tree construction to get a canonical Huffman tree (longest code
       * all ones)
       */
      gen_codes(static_ltree, L_CODES$1 + 1, bl_count);

      /* The static distance tree is trivial: */
      for (n = 0; n < D_CODES$1; n++) {
        static_dtree[n * 2 + 1]/*.Len*/ = 5;
        static_dtree[n * 2]/*.Code*/ = bi_reverse(n, 5);
      }

      // Now data ready and we can init static trees
      static_l_desc = new StaticTreeDesc(static_ltree, extra_lbits, LITERALS$1 + 1, L_CODES$1, MAX_BITS$1);
      static_d_desc = new StaticTreeDesc(static_dtree, extra_dbits, 0,          D_CODES$1, MAX_BITS$1);
      static_bl_desc = new StaticTreeDesc(new Array(0), extra_blbits, 0,         BL_CODES$1, MAX_BL_BITS);

      //static_init_done = true;
    };


    /* ===========================================================================
     * Initialize a new block.
     */
    const init_block = (s) => {

      let n; /* iterates over tree elements */

      /* Initialize the trees. */
      for (n = 0; n < L_CODES$1;  n++) { s.dyn_ltree[n * 2]/*.Freq*/ = 0; }
      for (n = 0; n < D_CODES$1;  n++) { s.dyn_dtree[n * 2]/*.Freq*/ = 0; }
      for (n = 0; n < BL_CODES$1; n++) { s.bl_tree[n * 2]/*.Freq*/ = 0; }

      s.dyn_ltree[END_BLOCK * 2]/*.Freq*/ = 1;
      s.opt_len = s.static_len = 0;
      s.sym_next = s.matches = 0;
    };


    /* ===========================================================================
     * Flush the bit buffer and align the output on a byte boundary
     */
    const bi_windup = (s) =>
    {
      if (s.bi_valid > 8) {
        put_short(s, s.bi_buf);
      } else if (s.bi_valid > 0) {
        //put_byte(s, (Byte)s->bi_buf);
        s.pending_buf[s.pending++] = s.bi_buf;
      }
      s.bi_buf = 0;
      s.bi_valid = 0;
    };

    /* ===========================================================================
     * Compares to subtrees, using the tree depth as tie breaker when
     * the subtrees have equal frequency. This minimizes the worst case length.
     */
    const smaller = (tree, n, m, depth) => {

      const _n2 = n * 2;
      const _m2 = m * 2;
      return (tree[_n2]/*.Freq*/ < tree[_m2]/*.Freq*/ ||
             (tree[_n2]/*.Freq*/ === tree[_m2]/*.Freq*/ && depth[n] <= depth[m]));
    };

    /* ===========================================================================
     * Restore the heap property by moving down the tree starting at node k,
     * exchanging a node with the smallest of its two sons if necessary, stopping
     * when the heap property is re-established (each father smaller than its
     * two sons).
     */
    const pqdownheap = (s, tree, k) => {
    //    deflate_state *s;
    //    ct_data *tree;  /* the tree to restore */
    //    int k;               /* node to move down */

      const v = s.heap[k];
      let j = k << 1;  /* left son of k */
      while (j <= s.heap_len) {
        /* Set j to the smallest of the two sons: */
        if (j < s.heap_len &&
          smaller(tree, s.heap[j + 1], s.heap[j], s.depth)) {
          j++;
        }
        /* Exit if v is smaller than both sons */
        if (smaller(tree, v, s.heap[j], s.depth)) { break; }

        /* Exchange v with the smallest son */
        s.heap[k] = s.heap[j];
        k = j;

        /* And continue down the tree, setting j to the left son of k */
        j <<= 1;
      }
      s.heap[k] = v;
    };


    // inlined manually
    // const SMALLEST = 1;

    /* ===========================================================================
     * Send the block data compressed using the given Huffman trees
     */
    const compress_block = (s, ltree, dtree) => {
    //    deflate_state *s;
    //    const ct_data *ltree; /* literal tree */
    //    const ct_data *dtree; /* distance tree */

      let dist;           /* distance of matched string */
      let lc;             /* match length or unmatched char (if dist == 0) */
      let sx = 0;         /* running index in sym_buf */
      let code;           /* the code to send */
      let extra;          /* number of extra bits to send */

      if (s.sym_next !== 0) {
        do {
          dist = s.pending_buf[s.sym_buf + sx++] & 0xff;
          dist += (s.pending_buf[s.sym_buf + sx++] & 0xff) << 8;
          lc = s.pending_buf[s.sym_buf + sx++];
          if (dist === 0) {
            send_code(s, lc, ltree); /* send a literal byte */
            //Tracecv(isgraph(lc), (stderr," '%c' ", lc));
          } else {
            /* Here, lc is the match length - MIN_MATCH */
            code = _length_code[lc];
            send_code(s, code + LITERALS$1 + 1, ltree); /* send the length code */
            extra = extra_lbits[code];
            if (extra !== 0) {
              lc -= base_length[code];
              send_bits(s, lc, extra);       /* send the extra length bits */
            }
            dist--; /* dist is now the match distance - 1 */
            code = d_code(dist);
            //Assert (code < D_CODES, "bad d_code");

            send_code(s, code, dtree);       /* send the distance code */
            extra = extra_dbits[code];
            if (extra !== 0) {
              dist -= base_dist[code];
              send_bits(s, dist, extra);   /* send the extra distance bits */
            }
          } /* literal or match pair ? */

          /* Check that the overlay between pending_buf and sym_buf is ok: */
          //Assert(s->pending < s->lit_bufsize + sx, "pendingBuf overflow");

        } while (sx < s.sym_next);
      }

      send_code(s, END_BLOCK, ltree);
    };


    /* ===========================================================================
     * Construct one Huffman tree and assigns the code bit strings and lengths.
     * Update the total bit length for the current block.
     * IN assertion: the field freq is set for all tree elements.
     * OUT assertions: the fields len and code are set to the optimal bit length
     *     and corresponding code. The length opt_len is updated; static_len is
     *     also updated if stree is not null. The field max_code is set.
     */
    const build_tree = (s, desc) => {
    //    deflate_state *s;
    //    tree_desc *desc; /* the tree descriptor */

      const tree     = desc.dyn_tree;
      const stree    = desc.stat_desc.static_tree;
      const has_stree = desc.stat_desc.has_stree;
      const elems    = desc.stat_desc.elems;
      let n, m;          /* iterate over heap elements */
      let max_code = -1; /* largest code with non zero frequency */
      let node;          /* new node being created */

      /* Construct the initial heap, with least frequent element in
       * heap[SMALLEST]. The sons of heap[n] are heap[2*n] and heap[2*n+1].
       * heap[0] is not used.
       */
      s.heap_len = 0;
      s.heap_max = HEAP_SIZE$1;

      for (n = 0; n < elems; n++) {
        if (tree[n * 2]/*.Freq*/ !== 0) {
          s.heap[++s.heap_len] = max_code = n;
          s.depth[n] = 0;

        } else {
          tree[n * 2 + 1]/*.Len*/ = 0;
        }
      }

      /* The pkzip format requires that at least one distance code exists,
       * and that at least one bit should be sent even if there is only one
       * possible code. So to avoid special checks later on we force at least
       * two codes of non zero frequency.
       */
      while (s.heap_len < 2) {
        node = s.heap[++s.heap_len] = (max_code < 2 ? ++max_code : 0);
        tree[node * 2]/*.Freq*/ = 1;
        s.depth[node] = 0;
        s.opt_len--;

        if (has_stree) {
          s.static_len -= stree[node * 2 + 1]/*.Len*/;
        }
        /* node is 0 or 1 so it does not have extra bits */
      }
      desc.max_code = max_code;

      /* The elements heap[heap_len/2+1 .. heap_len] are leaves of the tree,
       * establish sub-heaps of increasing lengths:
       */
      for (n = (s.heap_len >> 1/*int /2*/); n >= 1; n--) { pqdownheap(s, tree, n); }

      /* Construct the Huffman tree by repeatedly combining the least two
       * frequent nodes.
       */
      node = elems;              /* next internal node of the tree */
      do {
        //pqremove(s, tree, n);  /* n = node of least frequency */
        /*** pqremove ***/
        n = s.heap[1/*SMALLEST*/];
        s.heap[1/*SMALLEST*/] = s.heap[s.heap_len--];
        pqdownheap(s, tree, 1/*SMALLEST*/);
        /***/

        m = s.heap[1/*SMALLEST*/]; /* m = node of next least frequency */

        s.heap[--s.heap_max] = n; /* keep the nodes sorted by frequency */
        s.heap[--s.heap_max] = m;

        /* Create a new node father of n and m */
        tree[node * 2]/*.Freq*/ = tree[n * 2]/*.Freq*/ + tree[m * 2]/*.Freq*/;
        s.depth[node] = (s.depth[n] >= s.depth[m] ? s.depth[n] : s.depth[m]) + 1;
        tree[n * 2 + 1]/*.Dad*/ = tree[m * 2 + 1]/*.Dad*/ = node;

        /* and insert the new node in the heap */
        s.heap[1/*SMALLEST*/] = node++;
        pqdownheap(s, tree, 1/*SMALLEST*/);

      } while (s.heap_len >= 2);

      s.heap[--s.heap_max] = s.heap[1/*SMALLEST*/];

      /* At this point, the fields freq and dad are set. We can now
       * generate the bit lengths.
       */
      gen_bitlen(s, desc);

      /* The field len is now set, we can generate the bit codes */
      gen_codes(tree, max_code, s.bl_count);
    };


    /* ===========================================================================
     * Scan a literal or distance tree to determine the frequencies of the codes
     * in the bit length tree.
     */
    const scan_tree = (s, tree, max_code) => {
    //    deflate_state *s;
    //    ct_data *tree;   /* the tree to be scanned */
    //    int max_code;    /* and its largest code of non zero frequency */

      let n;                     /* iterates over all tree elements */
      let prevlen = -1;          /* last emitted length */
      let curlen;                /* length of current code */

      let nextlen = tree[0 * 2 + 1]/*.Len*/; /* length of next code */

      let count = 0;             /* repeat count of the current code */
      let max_count = 7;         /* max repeat count */
      let min_count = 4;         /* min repeat count */

      if (nextlen === 0) {
        max_count = 138;
        min_count = 3;
      }
      tree[(max_code + 1) * 2 + 1]/*.Len*/ = 0xffff; /* guard */

      for (n = 0; n <= max_code; n++) {
        curlen = nextlen;
        nextlen = tree[(n + 1) * 2 + 1]/*.Len*/;

        if (++count < max_count && curlen === nextlen) {
          continue;

        } else if (count < min_count) {
          s.bl_tree[curlen * 2]/*.Freq*/ += count;

        } else if (curlen !== 0) {

          if (curlen !== prevlen) { s.bl_tree[curlen * 2]/*.Freq*/++; }
          s.bl_tree[REP_3_6 * 2]/*.Freq*/++;

        } else if (count <= 10) {
          s.bl_tree[REPZ_3_10 * 2]/*.Freq*/++;

        } else {
          s.bl_tree[REPZ_11_138 * 2]/*.Freq*/++;
        }

        count = 0;
        prevlen = curlen;

        if (nextlen === 0) {
          max_count = 138;
          min_count = 3;

        } else if (curlen === nextlen) {
          max_count = 6;
          min_count = 3;

        } else {
          max_count = 7;
          min_count = 4;
        }
      }
    };


    /* ===========================================================================
     * Send a literal or distance tree in compressed form, using the codes in
     * bl_tree.
     */
    const send_tree = (s, tree, max_code) => {
    //    deflate_state *s;
    //    ct_data *tree; /* the tree to be scanned */
    //    int max_code;       /* and its largest code of non zero frequency */

      let n;                     /* iterates over all tree elements */
      let prevlen = -1;          /* last emitted length */
      let curlen;                /* length of current code */

      let nextlen = tree[0 * 2 + 1]/*.Len*/; /* length of next code */

      let count = 0;             /* repeat count of the current code */
      let max_count = 7;         /* max repeat count */
      let min_count = 4;         /* min repeat count */

      /* tree[max_code+1].Len = -1; */  /* guard already set */
      if (nextlen === 0) {
        max_count = 138;
        min_count = 3;
      }

      for (n = 0; n <= max_code; n++) {
        curlen = nextlen;
        nextlen = tree[(n + 1) * 2 + 1]/*.Len*/;

        if (++count < max_count && curlen === nextlen) {
          continue;

        } else if (count < min_count) {
          do { send_code(s, curlen, s.bl_tree); } while (--count !== 0);

        } else if (curlen !== 0) {
          if (curlen !== prevlen) {
            send_code(s, curlen, s.bl_tree);
            count--;
          }
          //Assert(count >= 3 && count <= 6, " 3_6?");
          send_code(s, REP_3_6, s.bl_tree);
          send_bits(s, count - 3, 2);

        } else if (count <= 10) {
          send_code(s, REPZ_3_10, s.bl_tree);
          send_bits(s, count - 3, 3);

        } else {
          send_code(s, REPZ_11_138, s.bl_tree);
          send_bits(s, count - 11, 7);
        }

        count = 0;
        prevlen = curlen;
        if (nextlen === 0) {
          max_count = 138;
          min_count = 3;

        } else if (curlen === nextlen) {
          max_count = 6;
          min_count = 3;

        } else {
          max_count = 7;
          min_count = 4;
        }
      }
    };


    /* ===========================================================================
     * Construct the Huffman tree for the bit lengths and return the index in
     * bl_order of the last bit length code to send.
     */
    const build_bl_tree = (s) => {

      let max_blindex;  /* index of last bit length code of non zero freq */

      /* Determine the bit length frequencies for literal and distance trees */
      scan_tree(s, s.dyn_ltree, s.l_desc.max_code);
      scan_tree(s, s.dyn_dtree, s.d_desc.max_code);

      /* Build the bit length tree: */
      build_tree(s, s.bl_desc);
      /* opt_len now includes the length of the tree representations, except
       * the lengths of the bit lengths codes and the 5+5+4 bits for the counts.
       */

      /* Determine the number of bit length codes to send. The pkzip format
       * requires that at least 4 bit length codes be sent. (appnote.txt says
       * 3 but the actual value used is 4.)
       */
      for (max_blindex = BL_CODES$1 - 1; max_blindex >= 3; max_blindex--) {
        if (s.bl_tree[bl_order[max_blindex] * 2 + 1]/*.Len*/ !== 0) {
          break;
        }
      }
      /* Update opt_len to include the bit length tree and counts */
      s.opt_len += 3 * (max_blindex + 1) + 5 + 5 + 4;
      //Tracev((stderr, "\ndyn trees: dyn %ld, stat %ld",
      //        s->opt_len, s->static_len));

      return max_blindex;
    };


    /* ===========================================================================
     * Send the header for a block using dynamic Huffman trees: the counts, the
     * lengths of the bit length codes, the literal tree and the distance tree.
     * IN assertion: lcodes >= 257, dcodes >= 1, blcodes >= 4.
     */
    const send_all_trees = (s, lcodes, dcodes, blcodes) => {
    //    deflate_state *s;
    //    int lcodes, dcodes, blcodes; /* number of codes for each tree */

      let rank;                    /* index in bl_order */

      //Assert (lcodes >= 257 && dcodes >= 1 && blcodes >= 4, "not enough codes");
      //Assert (lcodes <= L_CODES && dcodes <= D_CODES && blcodes <= BL_CODES,
      //        "too many codes");
      //Tracev((stderr, "\nbl counts: "));
      send_bits(s, lcodes - 257, 5); /* not +255 as stated in appnote.txt */
      send_bits(s, dcodes - 1,   5);
      send_bits(s, blcodes - 4,  4); /* not -3 as stated in appnote.txt */
      for (rank = 0; rank < blcodes; rank++) {
        //Tracev((stderr, "\nbl code %2d ", bl_order[rank]));
        send_bits(s, s.bl_tree[bl_order[rank] * 2 + 1]/*.Len*/, 3);
      }
      //Tracev((stderr, "\nbl tree: sent %ld", s->bits_sent));

      send_tree(s, s.dyn_ltree, lcodes - 1); /* literal tree */
      //Tracev((stderr, "\nlit tree: sent %ld", s->bits_sent));

      send_tree(s, s.dyn_dtree, dcodes - 1); /* distance tree */
      //Tracev((stderr, "\ndist tree: sent %ld", s->bits_sent));
    };


    /* ===========================================================================
     * Check if the data type is TEXT or BINARY, using the following algorithm:
     * - TEXT if the two conditions below are satisfied:
     *    a) There are no non-portable control characters belonging to the
     *       "block list" (0..6, 14..25, 28..31).
     *    b) There is at least one printable character belonging to the
     *       "allow list" (9 {TAB}, 10 {LF}, 13 {CR}, 32..255).
     * - BINARY otherwise.
     * - The following partially-portable control characters form a
     *   "gray list" that is ignored in this detection algorithm:
     *   (7 {BEL}, 8 {BS}, 11 {VT}, 12 {FF}, 26 {SUB}, 27 {ESC}).
     * IN assertion: the fields Freq of dyn_ltree are set.
     */
    const detect_data_type = (s) => {
      /* block_mask is the bit mask of block-listed bytes
       * set bits 0..6, 14..25, and 28..31
       * 0xf3ffc07f = binary 11110011111111111100000001111111
       */
      let block_mask = 0xf3ffc07f;
      let n;

      /* Check for non-textual ("block-listed") bytes. */
      for (n = 0; n <= 31; n++, block_mask >>>= 1) {
        if ((block_mask & 1) && (s.dyn_ltree[n * 2]/*.Freq*/ !== 0)) {
          return Z_BINARY;
        }
      }

      /* Check for textual ("allow-listed") bytes. */
      if (s.dyn_ltree[9 * 2]/*.Freq*/ !== 0 || s.dyn_ltree[10 * 2]/*.Freq*/ !== 0 ||
          s.dyn_ltree[13 * 2]/*.Freq*/ !== 0) {
        return Z_TEXT;
      }
      for (n = 32; n < LITERALS$1; n++) {
        if (s.dyn_ltree[n * 2]/*.Freq*/ !== 0) {
          return Z_TEXT;
        }
      }

      /* There are no "block-listed" or "allow-listed" bytes:
       * this stream either is empty or has tolerated ("gray-listed") bytes only.
       */
      return Z_BINARY;
    };


    let static_init_done = false;

    /* ===========================================================================
     * Initialize the tree data structures for a new zlib stream.
     */
    const _tr_init$1 = (s) =>
    {

      if (!static_init_done) {
        tr_static_init();
        static_init_done = true;
      }

      s.l_desc  = new TreeDesc(s.dyn_ltree, static_l_desc);
      s.d_desc  = new TreeDesc(s.dyn_dtree, static_d_desc);
      s.bl_desc = new TreeDesc(s.bl_tree, static_bl_desc);

      s.bi_buf = 0;
      s.bi_valid = 0;

      /* Initialize the first block of the first file: */
      init_block(s);
    };


    /* ===========================================================================
     * Send a stored block
     */
    const _tr_stored_block$1 = (s, buf, stored_len, last) => {
    //DeflateState *s;
    //charf *buf;       /* input block */
    //ulg stored_len;   /* length of input block */
    //int last;         /* one if this is the last block for a file */

      send_bits(s, (STORED_BLOCK << 1) + (last ? 1 : 0), 3);    /* send block type */
      bi_windup(s);        /* align on byte boundary */
      put_short(s, stored_len);
      put_short(s, ~stored_len);
      if (stored_len) {
        s.pending_buf.set(s.window.subarray(buf, buf + stored_len), s.pending);
      }
      s.pending += stored_len;
    };


    /* ===========================================================================
     * Send one empty static block to give enough lookahead for inflate.
     * This takes 10 bits, of which 7 may remain in the bit buffer.
     */
    const _tr_align$1 = (s) => {
      send_bits(s, STATIC_TREES << 1, 3);
      send_code(s, END_BLOCK, static_ltree);
      bi_flush(s);
    };


    /* ===========================================================================
     * Determine the best encoding for the current block: dynamic trees, static
     * trees or store, and write out the encoded block.
     */
    const _tr_flush_block$1 = (s, buf, stored_len, last) => {
    //DeflateState *s;
    //charf *buf;       /* input block, or NULL if too old */
    //ulg stored_len;   /* length of input block */
    //int last;         /* one if this is the last block for a file */

      let opt_lenb, static_lenb;  /* opt_len and static_len in bytes */
      let max_blindex = 0;        /* index of last bit length code of non zero freq */

      /* Build the Huffman trees unless a stored block is forced */
      if (s.level > 0) {

        /* Check if the file is binary or text */
        if (s.strm.data_type === Z_UNKNOWN$1) {
          s.strm.data_type = detect_data_type(s);
        }

        /* Construct the literal and distance trees */
        build_tree(s, s.l_desc);
        // Tracev((stderr, "\nlit data: dyn %ld, stat %ld", s->opt_len,
        //        s->static_len));

        build_tree(s, s.d_desc);
        // Tracev((stderr, "\ndist data: dyn %ld, stat %ld", s->opt_len,
        //        s->static_len));
        /* At this point, opt_len and static_len are the total bit lengths of
         * the compressed block data, excluding the tree representations.
         */

        /* Build the bit length tree for the above two trees, and get the index
         * in bl_order of the last bit length code to send.
         */
        max_blindex = build_bl_tree(s);

        /* Determine the best encoding. Compute the block lengths in bytes. */
        opt_lenb = (s.opt_len + 3 + 7) >>> 3;
        static_lenb = (s.static_len + 3 + 7) >>> 3;

        // Tracev((stderr, "\nopt %lu(%lu) stat %lu(%lu) stored %lu lit %u ",
        //        opt_lenb, s->opt_len, static_lenb, s->static_len, stored_len,
        //        s->sym_next / 3));

        if (static_lenb <= opt_lenb) { opt_lenb = static_lenb; }

      } else {
        // Assert(buf != (char*)0, "lost buf");
        opt_lenb = static_lenb = stored_len + 5; /* force a stored block */
      }

      if ((stored_len + 4 <= opt_lenb) && (buf !== -1)) {
        /* 4: two words for the lengths */

        /* The test buf != NULL is only necessary if LIT_BUFSIZE > WSIZE.
         * Otherwise we can't have processed more than WSIZE input bytes since
         * the last block flush, because compression would have been
         * successful. If LIT_BUFSIZE <= WSIZE, it is never too late to
         * transform a block into a stored block.
         */
        _tr_stored_block$1(s, buf, stored_len, last);

      } else if (s.strategy === Z_FIXED$1 || static_lenb === opt_lenb) {

        send_bits(s, (STATIC_TREES << 1) + (last ? 1 : 0), 3);
        compress_block(s, static_ltree, static_dtree);

      } else {
        send_bits(s, (DYN_TREES << 1) + (last ? 1 : 0), 3);
        send_all_trees(s, s.l_desc.max_code + 1, s.d_desc.max_code + 1, max_blindex + 1);
        compress_block(s, s.dyn_ltree, s.dyn_dtree);
      }
      // Assert (s->compressed_len == s->bits_sent, "bad compressed size");
      /* The above check is made mod 2^32, for files larger than 512 MB
       * and uLong implemented on 32 bits.
       */
      init_block(s);

      if (last) {
        bi_windup(s);
      }
      // Tracev((stderr,"\ncomprlen %lu(%lu) ", s->compressed_len>>3,
      //       s->compressed_len-7*last));
    };

    /* ===========================================================================
     * Save the match info and tally the frequency counts. Return true if
     * the current block must be flushed.
     */
    const _tr_tally$1 = (s, dist, lc) => {
    //    deflate_state *s;
    //    unsigned dist;  /* distance of matched string */
    //    unsigned lc;    /* match length-MIN_MATCH or unmatched char (if dist==0) */

      s.pending_buf[s.sym_buf + s.sym_next++] = dist;
      s.pending_buf[s.sym_buf + s.sym_next++] = dist >> 8;
      s.pending_buf[s.sym_buf + s.sym_next++] = lc;
      if (dist === 0) {
        /* lc is the unmatched char */
        s.dyn_ltree[lc * 2]/*.Freq*/++;
      } else {
        s.matches++;
        /* Here, lc is the match length - MIN_MATCH */
        dist--;             /* dist = match distance - 1 */
        //Assert((ush)dist < (ush)MAX_DIST(s) &&
        //       (ush)lc <= (ush)(MAX_MATCH-MIN_MATCH) &&
        //       (ush)d_code(dist) < (ush)D_CODES,  "_tr_tally: bad match");

        s.dyn_ltree[(_length_code[lc] + LITERALS$1 + 1) * 2]/*.Freq*/++;
        s.dyn_dtree[d_code(dist) * 2]/*.Freq*/++;
      }

      return (s.sym_next === s.sym_end);
    };

    var _tr_init_1  = _tr_init$1;
    var _tr_stored_block_1 = _tr_stored_block$1;
    var _tr_flush_block_1  = _tr_flush_block$1;
    var _tr_tally_1 = _tr_tally$1;
    var _tr_align_1 = _tr_align$1;

    var trees = {
    	_tr_init: _tr_init_1,
    	_tr_stored_block: _tr_stored_block_1,
    	_tr_flush_block: _tr_flush_block_1,
    	_tr_tally: _tr_tally_1,
    	_tr_align: _tr_align_1
    };

    // Note: adler32 takes 12% for level 0 and 2% for level 6.
    // It isn't worth it to make additional optimizations as in original.
    // Small size is preferable.

    // (C) 1995-2013 Jean-loup Gailly and Mark Adler
    // (C) 2014-2017 Vitaly Puzrin and Andrey Tupitsin
    //
    // This software is provided 'as-is', without any express or implied
    // warranty. In no event will the authors be held liable for any damages
    // arising from the use of this software.
    //
    // Permission is granted to anyone to use this software for any purpose,
    // including commercial applications, and to alter it and redistribute it
    // freely, subject to the following restrictions:
    //
    // 1. The origin of this software must not be misrepresented; you must not
    //   claim that you wrote the original software. If you use this software
    //   in a product, an acknowledgment in the product documentation would be
    //   appreciated but is not required.
    // 2. Altered source versions must be plainly marked as such, and must not be
    //   misrepresented as being the original software.
    // 3. This notice may not be removed or altered from any source distribution.

    const adler32 = (adler, buf, len, pos) => {
      let s1 = (adler & 0xffff) |0,
          s2 = ((adler >>> 16) & 0xffff) |0,
          n = 0;

      while (len !== 0) {
        // Set limit ~ twice less than 5552, to keep
        // s2 in 31-bits, because we force signed ints.
        // in other case %= will fail.
        n = len > 2000 ? 2000 : len;
        len -= n;

        do {
          s1 = (s1 + buf[pos++]) |0;
          s2 = (s2 + s1) |0;
        } while (--n);

        s1 %= 65521;
        s2 %= 65521;
      }

      return (s1 | (s2 << 16)) |0;
    };


    var adler32_1 = adler32;

    // Note: we can't get significant speed boost here.
    // So write code to minimize size - no pregenerated tables
    // and array tools dependencies.

    // (C) 1995-2013 Jean-loup Gailly and Mark Adler
    // (C) 2014-2017 Vitaly Puzrin and Andrey Tupitsin
    //
    // This software is provided 'as-is', without any express or implied
    // warranty. In no event will the authors be held liable for any damages
    // arising from the use of this software.
    //
    // Permission is granted to anyone to use this software for any purpose,
    // including commercial applications, and to alter it and redistribute it
    // freely, subject to the following restrictions:
    //
    // 1. The origin of this software must not be misrepresented; you must not
    //   claim that you wrote the original software. If you use this software
    //   in a product, an acknowledgment in the product documentation would be
    //   appreciated but is not required.
    // 2. Altered source versions must be plainly marked as such, and must not be
    //   misrepresented as being the original software.
    // 3. This notice may not be removed or altered from any source distribution.

    // Use ordinary array, since untyped makes no boost here
    const makeTable = () => {
      let c, table = [];

      for (var n = 0; n < 256; n++) {
        c = n;
        for (var k = 0; k < 8; k++) {
          c = ((c & 1) ? (0xEDB88320 ^ (c >>> 1)) : (c >>> 1));
        }
        table[n] = c;
      }

      return table;
    };

    // Create table on load. Just 255 signed longs. Not a problem.
    const crcTable = new Uint32Array(makeTable());


    const crc32 = (crc, buf, len, pos) => {
      const t = crcTable;
      const end = pos + len;

      crc ^= -1;

      for (let i = pos; i < end; i++) {
        crc = (crc >>> 8) ^ t[(crc ^ buf[i]) & 0xFF];
      }

      return (crc ^ (-1)); // >>> 0;
    };


    var crc32_1 = crc32;

    // (C) 1995-2013 Jean-loup Gailly and Mark Adler
    // (C) 2014-2017 Vitaly Puzrin and Andrey Tupitsin
    //
    // This software is provided 'as-is', without any express or implied
    // warranty. In no event will the authors be held liable for any damages
    // arising from the use of this software.
    //
    // Permission is granted to anyone to use this software for any purpose,
    // including commercial applications, and to alter it and redistribute it
    // freely, subject to the following restrictions:
    //
    // 1. The origin of this software must not be misrepresented; you must not
    //   claim that you wrote the original software. If you use this software
    //   in a product, an acknowledgment in the product documentation would be
    //   appreciated but is not required.
    // 2. Altered source versions must be plainly marked as such, and must not be
    //   misrepresented as being the original software.
    // 3. This notice may not be removed or altered from any source distribution.

    var messages = {
      2:      'need dictionary',     /* Z_NEED_DICT       2  */
      1:      'stream end',          /* Z_STREAM_END      1  */
      0:      '',                    /* Z_OK              0  */
      '-1':   'file error',          /* Z_ERRNO         (-1) */
      '-2':   'stream error',        /* Z_STREAM_ERROR  (-2) */
      '-3':   'data error',          /* Z_DATA_ERROR    (-3) */
      '-4':   'insufficient memory', /* Z_MEM_ERROR     (-4) */
      '-5':   'buffer error',        /* Z_BUF_ERROR     (-5) */
      '-6':   'incompatible version' /* Z_VERSION_ERROR (-6) */
    };

    // (C) 1995-2013 Jean-loup Gailly and Mark Adler
    // (C) 2014-2017 Vitaly Puzrin and Andrey Tupitsin
    //
    // This software is provided 'as-is', without any express or implied
    // warranty. In no event will the authors be held liable for any damages
    // arising from the use of this software.
    //
    // Permission is granted to anyone to use this software for any purpose,
    // including commercial applications, and to alter it and redistribute it
    // freely, subject to the following restrictions:
    //
    // 1. The origin of this software must not be misrepresented; you must not
    //   claim that you wrote the original software. If you use this software
    //   in a product, an acknowledgment in the product documentation would be
    //   appreciated but is not required.
    // 2. Altered source versions must be plainly marked as such, and must not be
    //   misrepresented as being the original software.
    // 3. This notice may not be removed or altered from any source distribution.

    var constants$2 = {

      /* Allowed flush values; see deflate() and inflate() below for details */
      Z_NO_FLUSH:         0,
      Z_PARTIAL_FLUSH:    1,
      Z_SYNC_FLUSH:       2,
      Z_FULL_FLUSH:       3,
      Z_FINISH:           4,
      Z_BLOCK:            5,
      Z_TREES:            6,

      /* Return codes for the compression/decompression functions. Negative values
      * are errors, positive values are used for special but normal events.
      */
      Z_OK:               0,
      Z_STREAM_END:       1,
      Z_NEED_DICT:        2,
      Z_ERRNO:           -1,
      Z_STREAM_ERROR:    -2,
      Z_DATA_ERROR:      -3,
      Z_MEM_ERROR:       -4,
      Z_BUF_ERROR:       -5,
      //Z_VERSION_ERROR: -6,

      /* compression levels */
      Z_NO_COMPRESSION:         0,
      Z_BEST_SPEED:             1,
      Z_BEST_COMPRESSION:       9,
      Z_DEFAULT_COMPRESSION:   -1,


      Z_FILTERED:               1,
      Z_HUFFMAN_ONLY:           2,
      Z_RLE:                    3,
      Z_FIXED:                  4,
      Z_DEFAULT_STRATEGY:       0,

      /* Possible values of the data_type field (though see inflate()) */
      Z_BINARY:                 0,
      Z_TEXT:                   1,
      //Z_ASCII:                1, // = Z_TEXT (deprecated)
      Z_UNKNOWN:                2,

      /* The deflate compression method */
      Z_DEFLATED:               8
      //Z_NULL:                 null // Use -1 or null inline, depending on var type
    };

    // (C) 1995-2013 Jean-loup Gailly and Mark Adler
    // (C) 2014-2017 Vitaly Puzrin and Andrey Tupitsin
    //
    // This software is provided 'as-is', without any express or implied
    // warranty. In no event will the authors be held liable for any damages
    // arising from the use of this software.
    //
    // Permission is granted to anyone to use this software for any purpose,
    // including commercial applications, and to alter it and redistribute it
    // freely, subject to the following restrictions:
    //
    // 1. The origin of this software must not be misrepresented; you must not
    //   claim that you wrote the original software. If you use this software
    //   in a product, an acknowledgment in the product documentation would be
    //   appreciated but is not required.
    // 2. Altered source versions must be plainly marked as such, and must not be
    //   misrepresented as being the original software.
    // 3. This notice may not be removed or altered from any source distribution.

    const { _tr_init, _tr_stored_block, _tr_flush_block, _tr_tally, _tr_align } = trees;




    /* Public constants ==========================================================*/
    /* ===========================================================================*/

    const {
      Z_NO_FLUSH: Z_NO_FLUSH$2, Z_PARTIAL_FLUSH, Z_FULL_FLUSH: Z_FULL_FLUSH$1, Z_FINISH: Z_FINISH$3, Z_BLOCK: Z_BLOCK$1,
      Z_OK: Z_OK$3, Z_STREAM_END: Z_STREAM_END$3, Z_STREAM_ERROR: Z_STREAM_ERROR$2, Z_DATA_ERROR: Z_DATA_ERROR$2, Z_BUF_ERROR: Z_BUF_ERROR$1,
      Z_DEFAULT_COMPRESSION: Z_DEFAULT_COMPRESSION$1,
      Z_FILTERED, Z_HUFFMAN_ONLY, Z_RLE, Z_FIXED, Z_DEFAULT_STRATEGY: Z_DEFAULT_STRATEGY$1,
      Z_UNKNOWN,
      Z_DEFLATED: Z_DEFLATED$2
    } = constants$2;

    /*============================================================================*/


    const MAX_MEM_LEVEL = 9;
    /* Maximum value for memLevel in deflateInit2 */
    const MAX_WBITS$1 = 15;
    /* 32K LZ77 window */
    const DEF_MEM_LEVEL = 8;


    const LENGTH_CODES  = 29;
    /* number of length codes, not counting the special END_BLOCK code */
    const LITERALS      = 256;
    /* number of literal bytes 0..255 */
    const L_CODES       = LITERALS + 1 + LENGTH_CODES;
    /* number of Literal or Length codes, including the END_BLOCK code */
    const D_CODES       = 30;
    /* number of distance codes */
    const BL_CODES      = 19;
    /* number of codes used to transfer the bit lengths */
    const HEAP_SIZE     = 2 * L_CODES + 1;
    /* maximum heap size */
    const MAX_BITS  = 15;
    /* All codes must not exceed MAX_BITS bits */

    const MIN_MATCH = 3;
    const MAX_MATCH = 258;
    const MIN_LOOKAHEAD = (MAX_MATCH + MIN_MATCH + 1);

    const PRESET_DICT = 0x20;

    const INIT_STATE    =  42;    /* zlib header -> BUSY_STATE */
    //#ifdef GZIP
    const GZIP_STATE    =  57;    /* gzip header -> BUSY_STATE | EXTRA_STATE */
    //#endif
    const EXTRA_STATE   =  69;    /* gzip extra block -> NAME_STATE */
    const NAME_STATE    =  73;    /* gzip file name -> COMMENT_STATE */
    const COMMENT_STATE =  91;    /* gzip comment -> HCRC_STATE */
    const HCRC_STATE    = 103;    /* gzip header CRC -> BUSY_STATE */
    const BUSY_STATE    = 113;    /* deflate -> FINISH_STATE */
    const FINISH_STATE  = 666;    /* stream complete */

    const BS_NEED_MORE      = 1; /* block not completed, need more input or more output */
    const BS_BLOCK_DONE     = 2; /* block flush performed */
    const BS_FINISH_STARTED = 3; /* finish started, need only more output at next deflate */
    const BS_FINISH_DONE    = 4; /* finish done, accept no more input or output */

    const OS_CODE = 0x03; // Unix :) . Don't detect, use this default.

    const err = (strm, errorCode) => {
      strm.msg = messages[errorCode];
      return errorCode;
    };

    const rank = (f) => {
      return ((f) * 2) - ((f) > 4 ? 9 : 0);
    };

    const zero = (buf) => {
      let len = buf.length; while (--len >= 0) { buf[len] = 0; }
    };

    /* ===========================================================================
     * Slide the hash table when sliding the window down (could be avoided with 32
     * bit values at the expense of memory usage). We slide even when level == 0 to
     * keep the hash table consistent if we switch back to level > 0 later.
     */
    const slide_hash = (s) => {
      let n, m;
      let p;
      let wsize = s.w_size;

      n = s.hash_size;
      p = n;
      do {
        m = s.head[--p];
        s.head[p] = (m >= wsize ? m - wsize : 0);
      } while (--n);
      n = wsize;
    //#ifndef FASTEST
      p = n;
      do {
        m = s.prev[--p];
        s.prev[p] = (m >= wsize ? m - wsize : 0);
        /* If n is not on any hash chain, prev[n] is garbage but
         * its value will never be used.
         */
      } while (--n);
    //#endif
    };

    /* eslint-disable new-cap */
    let HASH_ZLIB = (s, prev, data) => ((prev << s.hash_shift) ^ data) & s.hash_mask;
    // This hash causes less collisions, https://github.com/nodeca/pako/issues/135
    // But breaks binary compatibility
    //let HASH_FAST = (s, prev, data) => ((prev << 8) + (prev >> 8) + (data << 4)) & s.hash_mask;
    let HASH = HASH_ZLIB;


    /* =========================================================================
     * Flush as much pending output as possible. All deflate() output, except for
     * some deflate_stored() output, goes through this function so some
     * applications may wish to modify it to avoid allocating a large
     * strm->next_out buffer and copying into it. (See also read_buf()).
     */
    const flush_pending = (strm) => {
      const s = strm.state;

      //_tr_flush_bits(s);
      let len = s.pending;
      if (len > strm.avail_out) {
        len = strm.avail_out;
      }
      if (len === 0) { return; }

      strm.output.set(s.pending_buf.subarray(s.pending_out, s.pending_out + len), strm.next_out);
      strm.next_out  += len;
      s.pending_out  += len;
      strm.total_out += len;
      strm.avail_out -= len;
      s.pending      -= len;
      if (s.pending === 0) {
        s.pending_out = 0;
      }
    };


    const flush_block_only = (s, last) => {
      _tr_flush_block(s, (s.block_start >= 0 ? s.block_start : -1), s.strstart - s.block_start, last);
      s.block_start = s.strstart;
      flush_pending(s.strm);
    };


    const put_byte = (s, b) => {
      s.pending_buf[s.pending++] = b;
    };


    /* =========================================================================
     * Put a short in the pending buffer. The 16-bit value is put in MSB order.
     * IN assertion: the stream state is correct and there is enough room in
     * pending_buf.
     */
    const putShortMSB = (s, b) => {

      //  put_byte(s, (Byte)(b >> 8));
    //  put_byte(s, (Byte)(b & 0xff));
      s.pending_buf[s.pending++] = (b >>> 8) & 0xff;
      s.pending_buf[s.pending++] = b & 0xff;
    };


    /* ===========================================================================
     * Read a new buffer from the current input stream, update the adler32
     * and total number of bytes read.  All deflate() input goes through
     * this function so some applications may wish to modify it to avoid
     * allocating a large strm->input buffer and copying from it.
     * (See also flush_pending()).
     */
    const read_buf = (strm, buf, start, size) => {

      let len = strm.avail_in;

      if (len > size) { len = size; }
      if (len === 0) { return 0; }

      strm.avail_in -= len;

      // zmemcpy(buf, strm->next_in, len);
      buf.set(strm.input.subarray(strm.next_in, strm.next_in + len), start);
      if (strm.state.wrap === 1) {
        strm.adler = adler32_1(strm.adler, buf, len, start);
      }

      else if (strm.state.wrap === 2) {
        strm.adler = crc32_1(strm.adler, buf, len, start);
      }

      strm.next_in += len;
      strm.total_in += len;

      return len;
    };


    /* ===========================================================================
     * Set match_start to the longest match starting at the given string and
     * return its length. Matches shorter or equal to prev_length are discarded,
     * in which case the result is equal to prev_length and match_start is
     * garbage.
     * IN assertions: cur_match is the head of the hash chain for the current
     *   string (strstart) and its distance is <= MAX_DIST, and prev_length >= 1
     * OUT assertion: the match length is not greater than s->lookahead.
     */
    const longest_match = (s, cur_match) => {

      let chain_length = s.max_chain_length;      /* max hash chain length */
      let scan = s.strstart; /* current string */
      let match;                       /* matched string */
      let len;                           /* length of current match */
      let best_len = s.prev_length;              /* best match length so far */
      let nice_match = s.nice_match;             /* stop if match long enough */
      const limit = (s.strstart > (s.w_size - MIN_LOOKAHEAD)) ?
          s.strstart - (s.w_size - MIN_LOOKAHEAD) : 0/*NIL*/;

      const _win = s.window; // shortcut

      const wmask = s.w_mask;
      const prev  = s.prev;

      /* Stop when cur_match becomes <= limit. To simplify the code,
       * we prevent matches with the string of window index 0.
       */

      const strend = s.strstart + MAX_MATCH;
      let scan_end1  = _win[scan + best_len - 1];
      let scan_end   = _win[scan + best_len];

      /* The code is optimized for HASH_BITS >= 8 and MAX_MATCH-2 multiple of 16.
       * It is easy to get rid of this optimization if necessary.
       */
      // Assert(s->hash_bits >= 8 && MAX_MATCH == 258, "Code too clever");

      /* Do not waste too much time if we already have a good match: */
      if (s.prev_length >= s.good_match) {
        chain_length >>= 2;
      }
      /* Do not look for matches beyond the end of the input. This is necessary
       * to make deflate deterministic.
       */
      if (nice_match > s.lookahead) { nice_match = s.lookahead; }

      // Assert((ulg)s->strstart <= s->window_size-MIN_LOOKAHEAD, "need lookahead");

      do {
        // Assert(cur_match < s->strstart, "no future");
        match = cur_match;

        /* Skip to next match if the match length cannot increase
         * or if the match length is less than 2.  Note that the checks below
         * for insufficient lookahead only occur occasionally for performance
         * reasons.  Therefore uninitialized memory will be accessed, and
         * conditional jumps will be made that depend on those values.
         * However the length of the match is limited to the lookahead, so
         * the output of deflate is not affected by the uninitialized values.
         */

        if (_win[match + best_len]     !== scan_end  ||
            _win[match + best_len - 1] !== scan_end1 ||
            _win[match]                !== _win[scan] ||
            _win[++match]              !== _win[scan + 1]) {
          continue;
        }

        /* The check at best_len-1 can be removed because it will be made
         * again later. (This heuristic is not always a win.)
         * It is not necessary to compare scan[2] and match[2] since they
         * are always equal when the other bytes match, given that
         * the hash keys are equal and that HASH_BITS >= 8.
         */
        scan += 2;
        match++;
        // Assert(*scan == *match, "match[2]?");

        /* We check for insufficient lookahead only every 8th comparison;
         * the 256th check will be made at strstart+258.
         */
        do {
          /*jshint noempty:false*/
        } while (_win[++scan] === _win[++match] && _win[++scan] === _win[++match] &&
                 _win[++scan] === _win[++match] && _win[++scan] === _win[++match] &&
                 _win[++scan] === _win[++match] && _win[++scan] === _win[++match] &&
                 _win[++scan] === _win[++match] && _win[++scan] === _win[++match] &&
                 scan < strend);

        // Assert(scan <= s->window+(unsigned)(s->window_size-1), "wild scan");

        len = MAX_MATCH - (strend - scan);
        scan = strend - MAX_MATCH;

        if (len > best_len) {
          s.match_start = cur_match;
          best_len = len;
          if (len >= nice_match) {
            break;
          }
          scan_end1  = _win[scan + best_len - 1];
          scan_end   = _win[scan + best_len];
        }
      } while ((cur_match = prev[cur_match & wmask]) > limit && --chain_length !== 0);

      if (best_len <= s.lookahead) {
        return best_len;
      }
      return s.lookahead;
    };


    /* ===========================================================================
     * Fill the window when the lookahead becomes insufficient.
     * Updates strstart and lookahead.
     *
     * IN assertion: lookahead < MIN_LOOKAHEAD
     * OUT assertions: strstart <= window_size-MIN_LOOKAHEAD
     *    At least one byte has been read, or avail_in == 0; reads are
     *    performed for at least two bytes (required for the zip translate_eol
     *    option -- not supported here).
     */
    const fill_window = (s) => {

      const _w_size = s.w_size;
      let n, more, str;

      //Assert(s->lookahead < MIN_LOOKAHEAD, "already enough lookahead");

      do {
        more = s.window_size - s.lookahead - s.strstart;

        // JS ints have 32 bit, block below not needed
        /* Deal with !@#$% 64K limit: */
        //if (sizeof(int) <= 2) {
        //    if (more == 0 && s->strstart == 0 && s->lookahead == 0) {
        //        more = wsize;
        //
        //  } else if (more == (unsigned)(-1)) {
        //        /* Very unlikely, but possible on 16 bit machine if
        //         * strstart == 0 && lookahead == 1 (input done a byte at time)
        //         */
        //        more--;
        //    }
        //}


        /* If the window is almost full and there is insufficient lookahead,
         * move the upper half to the lower one to make room in the upper half.
         */
        if (s.strstart >= _w_size + (_w_size - MIN_LOOKAHEAD)) {

          s.window.set(s.window.subarray(_w_size, _w_size + _w_size - more), 0);
          s.match_start -= _w_size;
          s.strstart -= _w_size;
          /* we now have strstart >= MAX_DIST */
          s.block_start -= _w_size;
          if (s.insert > s.strstart) {
            s.insert = s.strstart;
          }
          slide_hash(s);
          more += _w_size;
        }
        if (s.strm.avail_in === 0) {
          break;
        }

        /* If there was no sliding:
         *    strstart <= WSIZE+MAX_DIST-1 && lookahead <= MIN_LOOKAHEAD - 1 &&
         *    more == window_size - lookahead - strstart
         * => more >= window_size - (MIN_LOOKAHEAD-1 + WSIZE + MAX_DIST-1)
         * => more >= window_size - 2*WSIZE + 2
         * In the BIG_MEM or MMAP case (not yet supported),
         *   window_size == input_size + MIN_LOOKAHEAD  &&
         *   strstart + s->lookahead <= input_size => more >= MIN_LOOKAHEAD.
         * Otherwise, window_size == 2*WSIZE so more >= 2.
         * If there was sliding, more >= WSIZE. So in all cases, more >= 2.
         */
        //Assert(more >= 2, "more < 2");
        n = read_buf(s.strm, s.window, s.strstart + s.lookahead, more);
        s.lookahead += n;

        /* Initialize the hash value now that we have some input: */
        if (s.lookahead + s.insert >= MIN_MATCH) {
          str = s.strstart - s.insert;
          s.ins_h = s.window[str];

          /* UPDATE_HASH(s, s->ins_h, s->window[str + 1]); */
          s.ins_h = HASH(s, s.ins_h, s.window[str + 1]);
    //#if MIN_MATCH != 3
    //        Call update_hash() MIN_MATCH-3 more times
    //#endif
          while (s.insert) {
            /* UPDATE_HASH(s, s->ins_h, s->window[str + MIN_MATCH-1]); */
            s.ins_h = HASH(s, s.ins_h, s.window[str + MIN_MATCH - 1]);

            s.prev[str & s.w_mask] = s.head[s.ins_h];
            s.head[s.ins_h] = str;
            str++;
            s.insert--;
            if (s.lookahead + s.insert < MIN_MATCH) {
              break;
            }
          }
        }
        /* If the whole input has less than MIN_MATCH bytes, ins_h is garbage,
         * but this is not important since only literal bytes will be emitted.
         */

      } while (s.lookahead < MIN_LOOKAHEAD && s.strm.avail_in !== 0);

      /* If the WIN_INIT bytes after the end of the current data have never been
       * written, then zero those bytes in order to avoid memory check reports of
       * the use of uninitialized (or uninitialised as Julian writes) bytes by
       * the longest match routines.  Update the high water mark for the next
       * time through here.  WIN_INIT is set to MAX_MATCH since the longest match
       * routines allow scanning to strstart + MAX_MATCH, ignoring lookahead.
       */
    //  if (s.high_water < s.window_size) {
    //    const curr = s.strstart + s.lookahead;
    //    let init = 0;
    //
    //    if (s.high_water < curr) {
    //      /* Previous high water mark below current data -- zero WIN_INIT
    //       * bytes or up to end of window, whichever is less.
    //       */
    //      init = s.window_size - curr;
    //      if (init > WIN_INIT)
    //        init = WIN_INIT;
    //      zmemzero(s->window + curr, (unsigned)init);
    //      s->high_water = curr + init;
    //    }
    //    else if (s->high_water < (ulg)curr + WIN_INIT) {
    //      /* High water mark at or above current data, but below current data
    //       * plus WIN_INIT -- zero out to current data plus WIN_INIT, or up
    //       * to end of window, whichever is less.
    //       */
    //      init = (ulg)curr + WIN_INIT - s->high_water;
    //      if (init > s->window_size - s->high_water)
    //        init = s->window_size - s->high_water;
    //      zmemzero(s->window + s->high_water, (unsigned)init);
    //      s->high_water += init;
    //    }
    //  }
    //
    //  Assert((ulg)s->strstart <= s->window_size - MIN_LOOKAHEAD,
    //    "not enough room for search");
    };

    /* ===========================================================================
     * Copy without compression as much as possible from the input stream, return
     * the current block state.
     *
     * In case deflateParams() is used to later switch to a non-zero compression
     * level, s->matches (otherwise unused when storing) keeps track of the number
     * of hash table slides to perform. If s->matches is 1, then one hash table
     * slide will be done when switching. If s->matches is 2, the maximum value
     * allowed here, then the hash table will be cleared, since two or more slides
     * is the same as a clear.
     *
     * deflate_stored() is written to minimize the number of times an input byte is
     * copied. It is most efficient with large input and output buffers, which
     * maximizes the opportunites to have a single copy from next_in to next_out.
     */
    const deflate_stored = (s, flush) => {

      /* Smallest worthy block size when not flushing or finishing. By default
       * this is 32K. This can be as small as 507 bytes for memLevel == 1. For
       * large input and output buffers, the stored block size will be larger.
       */
      let min_block = s.pending_buf_size - 5 > s.w_size ? s.w_size : s.pending_buf_size - 5;

      /* Copy as many min_block or larger stored blocks directly to next_out as
       * possible. If flushing, copy the remaining available input to next_out as
       * stored blocks, if there is enough space.
       */
      let len, left, have, last = 0;
      let used = s.strm.avail_in;
      do {
        /* Set len to the maximum size block that we can copy directly with the
         * available input data and output space. Set left to how much of that
         * would be copied from what's left in the window.
         */
        len = 65535/* MAX_STORED */;     /* maximum deflate stored block length */
        have = (s.bi_valid + 42) >> 3;     /* number of header bytes */
        if (s.strm.avail_out < have) {         /* need room for header */
          break;
        }
          /* maximum stored block length that will fit in avail_out: */
        have = s.strm.avail_out - have;
        left = s.strstart - s.block_start;  /* bytes left in window */
        if (len > left + s.strm.avail_in) {
          len = left + s.strm.avail_in;   /* limit len to the input */
        }
        if (len > have) {
          len = have;             /* limit len to the output */
        }

        /* If the stored block would be less than min_block in length, or if
         * unable to copy all of the available input when flushing, then try
         * copying to the window and the pending buffer instead. Also don't
         * write an empty block when flushing -- deflate() does that.
         */
        if (len < min_block && ((len === 0 && flush !== Z_FINISH$3) ||
                            flush === Z_NO_FLUSH$2 ||
                            len !== left + s.strm.avail_in)) {
          break;
        }

        /* Make a dummy stored block in pending to get the header bytes,
         * including any pending bits. This also updates the debugging counts.
         */
        last = flush === Z_FINISH$3 && len === left + s.strm.avail_in ? 1 : 0;
        _tr_stored_block(s, 0, 0, last);

        /* Replace the lengths in the dummy stored block with len. */
        s.pending_buf[s.pending - 4] = len;
        s.pending_buf[s.pending - 3] = len >> 8;
        s.pending_buf[s.pending - 2] = ~len;
        s.pending_buf[s.pending - 1] = ~len >> 8;

        /* Write the stored block header bytes. */
        flush_pending(s.strm);

    //#ifdef ZLIB_DEBUG
    //    /* Update debugging counts for the data about to be copied. */
    //    s->compressed_len += len << 3;
    //    s->bits_sent += len << 3;
    //#endif

        /* Copy uncompressed bytes from the window to next_out. */
        if (left) {
          if (left > len) {
            left = len;
          }
          //zmemcpy(s->strm->next_out, s->window + s->block_start, left);
          s.strm.output.set(s.window.subarray(s.block_start, s.block_start + left), s.strm.next_out);
          s.strm.next_out += left;
          s.strm.avail_out -= left;
          s.strm.total_out += left;
          s.block_start += left;
          len -= left;
        }

        /* Copy uncompressed bytes directly from next_in to next_out, updating
         * the check value.
         */
        if (len) {
          read_buf(s.strm, s.strm.output, s.strm.next_out, len);
          s.strm.next_out += len;
          s.strm.avail_out -= len;
          s.strm.total_out += len;
        }
      } while (last === 0);

      /* Update the sliding window with the last s->w_size bytes of the copied
       * data, or append all of the copied data to the existing window if less
       * than s->w_size bytes were copied. Also update the number of bytes to
       * insert in the hash tables, in the event that deflateParams() switches to
       * a non-zero compression level.
       */
      used -= s.strm.avail_in;    /* number of input bytes directly copied */
      if (used) {
        /* If any input was used, then no unused input remains in the window,
         * therefore s->block_start == s->strstart.
         */
        if (used >= s.w_size) {  /* supplant the previous history */
          s.matches = 2;     /* clear hash */
          //zmemcpy(s->window, s->strm->next_in - s->w_size, s->w_size);
          s.window.set(s.strm.input.subarray(s.strm.next_in - s.w_size, s.strm.next_in), 0);
          s.strstart = s.w_size;
          s.insert = s.strstart;
        }
        else {
          if (s.window_size - s.strstart <= used) {
            /* Slide the window down. */
            s.strstart -= s.w_size;
            //zmemcpy(s->window, s->window + s->w_size, s->strstart);
            s.window.set(s.window.subarray(s.w_size, s.w_size + s.strstart), 0);
            if (s.matches < 2) {
              s.matches++;   /* add a pending slide_hash() */
            }
            if (s.insert > s.strstart) {
              s.insert = s.strstart;
            }
          }
          //zmemcpy(s->window + s->strstart, s->strm->next_in - used, used);
          s.window.set(s.strm.input.subarray(s.strm.next_in - used, s.strm.next_in), s.strstart);
          s.strstart += used;
          s.insert += used > s.w_size - s.insert ? s.w_size - s.insert : used;
        }
        s.block_start = s.strstart;
      }
      if (s.high_water < s.strstart) {
        s.high_water = s.strstart;
      }

      /* If the last block was written to next_out, then done. */
      if (last) {
        return BS_FINISH_DONE;
      }

      /* If flushing and all input has been consumed, then done. */
      if (flush !== Z_NO_FLUSH$2 && flush !== Z_FINISH$3 &&
        s.strm.avail_in === 0 && s.strstart === s.block_start) {
        return BS_BLOCK_DONE;
      }

      /* Fill the window with any remaining input. */
      have = s.window_size - s.strstart;
      if (s.strm.avail_in > have && s.block_start >= s.w_size) {
        /* Slide the window down. */
        s.block_start -= s.w_size;
        s.strstart -= s.w_size;
        //zmemcpy(s->window, s->window + s->w_size, s->strstart);
        s.window.set(s.window.subarray(s.w_size, s.w_size + s.strstart), 0);
        if (s.matches < 2) {
          s.matches++;       /* add a pending slide_hash() */
        }
        have += s.w_size;      /* more space now */
        if (s.insert > s.strstart) {
          s.insert = s.strstart;
        }
      }
      if (have > s.strm.avail_in) {
        have = s.strm.avail_in;
      }
      if (have) {
        read_buf(s.strm, s.window, s.strstart, have);
        s.strstart += have;
        s.insert += have > s.w_size - s.insert ? s.w_size - s.insert : have;
      }
      if (s.high_water < s.strstart) {
        s.high_water = s.strstart;
      }

      /* There was not enough avail_out to write a complete worthy or flushed
       * stored block to next_out. Write a stored block to pending instead, if we
       * have enough input for a worthy block, or if flushing and there is enough
       * room for the remaining input as a stored block in the pending buffer.
       */
      have = (s.bi_valid + 42) >> 3;     /* number of header bytes */
        /* maximum stored block length that will fit in pending: */
      have = s.pending_buf_size - have > 65535/* MAX_STORED */ ? 65535/* MAX_STORED */ : s.pending_buf_size - have;
      min_block = have > s.w_size ? s.w_size : have;
      left = s.strstart - s.block_start;
      if (left >= min_block ||
         ((left || flush === Z_FINISH$3) && flush !== Z_NO_FLUSH$2 &&
         s.strm.avail_in === 0 && left <= have)) {
        len = left > have ? have : left;
        last = flush === Z_FINISH$3 && s.strm.avail_in === 0 &&
             len === left ? 1 : 0;
        _tr_stored_block(s, s.block_start, len, last);
        s.block_start += len;
        flush_pending(s.strm);
      }

      /* We've done all we can with the available input and output. */
      return last ? BS_FINISH_STARTED : BS_NEED_MORE;
    };


    /* ===========================================================================
     * Compress as much as possible from the input stream, return the current
     * block state.
     * This function does not perform lazy evaluation of matches and inserts
     * new strings in the dictionary only for unmatched strings or for short
     * matches. It is used only for the fast compression options.
     */
    const deflate_fast = (s, flush) => {

      let hash_head;        /* head of the hash chain */
      let bflush;           /* set if current block must be flushed */

      for (;;) {
        /* Make sure that we always have enough lookahead, except
         * at the end of the input file. We need MAX_MATCH bytes
         * for the next match, plus MIN_MATCH bytes to insert the
         * string following the next match.
         */
        if (s.lookahead < MIN_LOOKAHEAD) {
          fill_window(s);
          if (s.lookahead < MIN_LOOKAHEAD && flush === Z_NO_FLUSH$2) {
            return BS_NEED_MORE;
          }
          if (s.lookahead === 0) {
            break; /* flush the current block */
          }
        }

        /* Insert the string window[strstart .. strstart+2] in the
         * dictionary, and set hash_head to the head of the hash chain:
         */
        hash_head = 0/*NIL*/;
        if (s.lookahead >= MIN_MATCH) {
          /*** INSERT_STRING(s, s.strstart, hash_head); ***/
          s.ins_h = HASH(s, s.ins_h, s.window[s.strstart + MIN_MATCH - 1]);
          hash_head = s.prev[s.strstart & s.w_mask] = s.head[s.ins_h];
          s.head[s.ins_h] = s.strstart;
          /***/
        }

        /* Find the longest match, discarding those <= prev_length.
         * At this point we have always match_length < MIN_MATCH
         */
        if (hash_head !== 0/*NIL*/ && ((s.strstart - hash_head) <= (s.w_size - MIN_LOOKAHEAD))) {
          /* To simplify the code, we prevent matches with the string
           * of window index 0 (in particular we have to avoid a match
           * of the string with itself at the start of the input file).
           */
          s.match_length = longest_match(s, hash_head);
          /* longest_match() sets match_start */
        }
        if (s.match_length >= MIN_MATCH) {
          // check_match(s, s.strstart, s.match_start, s.match_length); // for debug only

          /*** _tr_tally_dist(s, s.strstart - s.match_start,
                         s.match_length - MIN_MATCH, bflush); ***/
          bflush = _tr_tally(s, s.strstart - s.match_start, s.match_length - MIN_MATCH);

          s.lookahead -= s.match_length;

          /* Insert new strings in the hash table only if the match length
           * is not too large. This saves time but degrades compression.
           */
          if (s.match_length <= s.max_lazy_match/*max_insert_length*/ && s.lookahead >= MIN_MATCH) {
            s.match_length--; /* string at strstart already in table */
            do {
              s.strstart++;
              /*** INSERT_STRING(s, s.strstart, hash_head); ***/
              s.ins_h = HASH(s, s.ins_h, s.window[s.strstart + MIN_MATCH - 1]);
              hash_head = s.prev[s.strstart & s.w_mask] = s.head[s.ins_h];
              s.head[s.ins_h] = s.strstart;
              /***/
              /* strstart never exceeds WSIZE-MAX_MATCH, so there are
               * always MIN_MATCH bytes ahead.
               */
            } while (--s.match_length !== 0);
            s.strstart++;
          } else
          {
            s.strstart += s.match_length;
            s.match_length = 0;
            s.ins_h = s.window[s.strstart];
            /* UPDATE_HASH(s, s.ins_h, s.window[s.strstart+1]); */
            s.ins_h = HASH(s, s.ins_h, s.window[s.strstart + 1]);

    //#if MIN_MATCH != 3
    //                Call UPDATE_HASH() MIN_MATCH-3 more times
    //#endif
            /* If lookahead < MIN_MATCH, ins_h is garbage, but it does not
             * matter since it will be recomputed at next deflate call.
             */
          }
        } else {
          /* No match, output a literal byte */
          //Tracevv((stderr,"%c", s.window[s.strstart]));
          /*** _tr_tally_lit(s, s.window[s.strstart], bflush); ***/
          bflush = _tr_tally(s, 0, s.window[s.strstart]);

          s.lookahead--;
          s.strstart++;
        }
        if (bflush) {
          /*** FLUSH_BLOCK(s, 0); ***/
          flush_block_only(s, false);
          if (s.strm.avail_out === 0) {
            return BS_NEED_MORE;
          }
          /***/
        }
      }
      s.insert = ((s.strstart < (MIN_MATCH - 1)) ? s.strstart : MIN_MATCH - 1);
      if (flush === Z_FINISH$3) {
        /*** FLUSH_BLOCK(s, 1); ***/
        flush_block_only(s, true);
        if (s.strm.avail_out === 0) {
          return BS_FINISH_STARTED;
        }
        /***/
        return BS_FINISH_DONE;
      }
      if (s.sym_next) {
        /*** FLUSH_BLOCK(s, 0); ***/
        flush_block_only(s, false);
        if (s.strm.avail_out === 0) {
          return BS_NEED_MORE;
        }
        /***/
      }
      return BS_BLOCK_DONE;
    };

    /* ===========================================================================
     * Same as above, but achieves better compression. We use a lazy
     * evaluation for matches: a match is finally adopted only if there is
     * no better match at the next window position.
     */
    const deflate_slow = (s, flush) => {

      let hash_head;          /* head of hash chain */
      let bflush;              /* set if current block must be flushed */

      let max_insert;

      /* Process the input block. */
      for (;;) {
        /* Make sure that we always have enough lookahead, except
         * at the end of the input file. We need MAX_MATCH bytes
         * for the next match, plus MIN_MATCH bytes to insert the
         * string following the next match.
         */
        if (s.lookahead < MIN_LOOKAHEAD) {
          fill_window(s);
          if (s.lookahead < MIN_LOOKAHEAD && flush === Z_NO_FLUSH$2) {
            return BS_NEED_MORE;
          }
          if (s.lookahead === 0) { break; } /* flush the current block */
        }

        /* Insert the string window[strstart .. strstart+2] in the
         * dictionary, and set hash_head to the head of the hash chain:
         */
        hash_head = 0/*NIL*/;
        if (s.lookahead >= MIN_MATCH) {
          /*** INSERT_STRING(s, s.strstart, hash_head); ***/
          s.ins_h = HASH(s, s.ins_h, s.window[s.strstart + MIN_MATCH - 1]);
          hash_head = s.prev[s.strstart & s.w_mask] = s.head[s.ins_h];
          s.head[s.ins_h] = s.strstart;
          /***/
        }

        /* Find the longest match, discarding those <= prev_length.
         */
        s.prev_length = s.match_length;
        s.prev_match = s.match_start;
        s.match_length = MIN_MATCH - 1;

        if (hash_head !== 0/*NIL*/ && s.prev_length < s.max_lazy_match &&
            s.strstart - hash_head <= (s.w_size - MIN_LOOKAHEAD)/*MAX_DIST(s)*/) {
          /* To simplify the code, we prevent matches with the string
           * of window index 0 (in particular we have to avoid a match
           * of the string with itself at the start of the input file).
           */
          s.match_length = longest_match(s, hash_head);
          /* longest_match() sets match_start */

          if (s.match_length <= 5 &&
             (s.strategy === Z_FILTERED || (s.match_length === MIN_MATCH && s.strstart - s.match_start > 4096/*TOO_FAR*/))) {

            /* If prev_match is also MIN_MATCH, match_start is garbage
             * but we will ignore the current match anyway.
             */
            s.match_length = MIN_MATCH - 1;
          }
        }
        /* If there was a match at the previous step and the current
         * match is not better, output the previous match:
         */
        if (s.prev_length >= MIN_MATCH && s.match_length <= s.prev_length) {
          max_insert = s.strstart + s.lookahead - MIN_MATCH;
          /* Do not insert strings in hash table beyond this. */

          //check_match(s, s.strstart-1, s.prev_match, s.prev_length);

          /***_tr_tally_dist(s, s.strstart - 1 - s.prev_match,
                         s.prev_length - MIN_MATCH, bflush);***/
          bflush = _tr_tally(s, s.strstart - 1 - s.prev_match, s.prev_length - MIN_MATCH);
          /* Insert in hash table all strings up to the end of the match.
           * strstart-1 and strstart are already inserted. If there is not
           * enough lookahead, the last two strings are not inserted in
           * the hash table.
           */
          s.lookahead -= s.prev_length - 1;
          s.prev_length -= 2;
          do {
            if (++s.strstart <= max_insert) {
              /*** INSERT_STRING(s, s.strstart, hash_head); ***/
              s.ins_h = HASH(s, s.ins_h, s.window[s.strstart + MIN_MATCH - 1]);
              hash_head = s.prev[s.strstart & s.w_mask] = s.head[s.ins_h];
              s.head[s.ins_h] = s.strstart;
              /***/
            }
          } while (--s.prev_length !== 0);
          s.match_available = 0;
          s.match_length = MIN_MATCH - 1;
          s.strstart++;

          if (bflush) {
            /*** FLUSH_BLOCK(s, 0); ***/
            flush_block_only(s, false);
            if (s.strm.avail_out === 0) {
              return BS_NEED_MORE;
            }
            /***/
          }

        } else if (s.match_available) {
          /* If there was no match at the previous position, output a
           * single literal. If there was a match but the current match
           * is longer, truncate the previous match to a single literal.
           */
          //Tracevv((stderr,"%c", s->window[s->strstart-1]));
          /*** _tr_tally_lit(s, s.window[s.strstart-1], bflush); ***/
          bflush = _tr_tally(s, 0, s.window[s.strstart - 1]);

          if (bflush) {
            /*** FLUSH_BLOCK_ONLY(s, 0) ***/
            flush_block_only(s, false);
            /***/
          }
          s.strstart++;
          s.lookahead--;
          if (s.strm.avail_out === 0) {
            return BS_NEED_MORE;
          }
        } else {
          /* There is no previous match to compare with, wait for
           * the next step to decide.
           */
          s.match_available = 1;
          s.strstart++;
          s.lookahead--;
        }
      }
      //Assert (flush != Z_NO_FLUSH, "no flush?");
      if (s.match_available) {
        //Tracevv((stderr,"%c", s->window[s->strstart-1]));
        /*** _tr_tally_lit(s, s.window[s.strstart-1], bflush); ***/
        bflush = _tr_tally(s, 0, s.window[s.strstart - 1]);

        s.match_available = 0;
      }
      s.insert = s.strstart < MIN_MATCH - 1 ? s.strstart : MIN_MATCH - 1;
      if (flush === Z_FINISH$3) {
        /*** FLUSH_BLOCK(s, 1); ***/
        flush_block_only(s, true);
        if (s.strm.avail_out === 0) {
          return BS_FINISH_STARTED;
        }
        /***/
        return BS_FINISH_DONE;
      }
      if (s.sym_next) {
        /*** FLUSH_BLOCK(s, 0); ***/
        flush_block_only(s, false);
        if (s.strm.avail_out === 0) {
          return BS_NEED_MORE;
        }
        /***/
      }

      return BS_BLOCK_DONE;
    };


    /* ===========================================================================
     * For Z_RLE, simply look for runs of bytes, generate matches only of distance
     * one.  Do not maintain a hash table.  (It will be regenerated if this run of
     * deflate switches away from Z_RLE.)
     */
    const deflate_rle = (s, flush) => {

      let bflush;            /* set if current block must be flushed */
      let prev;              /* byte at distance one to match */
      let scan, strend;      /* scan goes up to strend for length of run */

      const _win = s.window;

      for (;;) {
        /* Make sure that we always have enough lookahead, except
         * at the end of the input file. We need MAX_MATCH bytes
         * for the longest run, plus one for the unrolled loop.
         */
        if (s.lookahead <= MAX_MATCH) {
          fill_window(s);
          if (s.lookahead <= MAX_MATCH && flush === Z_NO_FLUSH$2) {
            return BS_NEED_MORE;
          }
          if (s.lookahead === 0) { break; } /* flush the current block */
        }

        /* See how many times the previous byte repeats */
        s.match_length = 0;
        if (s.lookahead >= MIN_MATCH && s.strstart > 0) {
          scan = s.strstart - 1;
          prev = _win[scan];
          if (prev === _win[++scan] && prev === _win[++scan] && prev === _win[++scan]) {
            strend = s.strstart + MAX_MATCH;
            do {
              /*jshint noempty:false*/
            } while (prev === _win[++scan] && prev === _win[++scan] &&
                     prev === _win[++scan] && prev === _win[++scan] &&
                     prev === _win[++scan] && prev === _win[++scan] &&
                     prev === _win[++scan] && prev === _win[++scan] &&
                     scan < strend);
            s.match_length = MAX_MATCH - (strend - scan);
            if (s.match_length > s.lookahead) {
              s.match_length = s.lookahead;
            }
          }
          //Assert(scan <= s->window+(uInt)(s->window_size-1), "wild scan");
        }

        /* Emit match if have run of MIN_MATCH or longer, else emit literal */
        if (s.match_length >= MIN_MATCH) {
          //check_match(s, s.strstart, s.strstart - 1, s.match_length);

          /*** _tr_tally_dist(s, 1, s.match_length - MIN_MATCH, bflush); ***/
          bflush = _tr_tally(s, 1, s.match_length - MIN_MATCH);

          s.lookahead -= s.match_length;
          s.strstart += s.match_length;
          s.match_length = 0;
        } else {
          /* No match, output a literal byte */
          //Tracevv((stderr,"%c", s->window[s->strstart]));
          /*** _tr_tally_lit(s, s.window[s.strstart], bflush); ***/
          bflush = _tr_tally(s, 0, s.window[s.strstart]);

          s.lookahead--;
          s.strstart++;
        }
        if (bflush) {
          /*** FLUSH_BLOCK(s, 0); ***/
          flush_block_only(s, false);
          if (s.strm.avail_out === 0) {
            return BS_NEED_MORE;
          }
          /***/
        }
      }
      s.insert = 0;
      if (flush === Z_FINISH$3) {
        /*** FLUSH_BLOCK(s, 1); ***/
        flush_block_only(s, true);
        if (s.strm.avail_out === 0) {
          return BS_FINISH_STARTED;
        }
        /***/
        return BS_FINISH_DONE;
      }
      if (s.sym_next) {
        /*** FLUSH_BLOCK(s, 0); ***/
        flush_block_only(s, false);
        if (s.strm.avail_out === 0) {
          return BS_NEED_MORE;
        }
        /***/
      }
      return BS_BLOCK_DONE;
    };

    /* ===========================================================================
     * For Z_HUFFMAN_ONLY, do not look for matches.  Do not maintain a hash table.
     * (It will be regenerated if this run of deflate switches away from Huffman.)
     */
    const deflate_huff = (s, flush) => {

      let bflush;             /* set if current block must be flushed */

      for (;;) {
        /* Make sure that we have a literal to write. */
        if (s.lookahead === 0) {
          fill_window(s);
          if (s.lookahead === 0) {
            if (flush === Z_NO_FLUSH$2) {
              return BS_NEED_MORE;
            }
            break;      /* flush the current block */
          }
        }

        /* Output a literal byte */
        s.match_length = 0;
        //Tracevv((stderr,"%c", s->window[s->strstart]));
        /*** _tr_tally_lit(s, s.window[s.strstart], bflush); ***/
        bflush = _tr_tally(s, 0, s.window[s.strstart]);
        s.lookahead--;
        s.strstart++;
        if (bflush) {
          /*** FLUSH_BLOCK(s, 0); ***/
          flush_block_only(s, false);
          if (s.strm.avail_out === 0) {
            return BS_NEED_MORE;
          }
          /***/
        }
      }
      s.insert = 0;
      if (flush === Z_FINISH$3) {
        /*** FLUSH_BLOCK(s, 1); ***/
        flush_block_only(s, true);
        if (s.strm.avail_out === 0) {
          return BS_FINISH_STARTED;
        }
        /***/
        return BS_FINISH_DONE;
      }
      if (s.sym_next) {
        /*** FLUSH_BLOCK(s, 0); ***/
        flush_block_only(s, false);
        if (s.strm.avail_out === 0) {
          return BS_NEED_MORE;
        }
        /***/
      }
      return BS_BLOCK_DONE;
    };

    /* Values for max_lazy_match, good_match and max_chain_length, depending on
     * the desired pack level (0..9). The values given below have been tuned to
     * exclude worst case performance for pathological files. Better values may be
     * found for specific files.
     */
    function Config(good_length, max_lazy, nice_length, max_chain, func) {

      this.good_length = good_length;
      this.max_lazy = max_lazy;
      this.nice_length = nice_length;
      this.max_chain = max_chain;
      this.func = func;
    }

    const configuration_table = [
      /*      good lazy nice chain */
      new Config(0, 0, 0, 0, deflate_stored),          /* 0 store only */
      new Config(4, 4, 8, 4, deflate_fast),            /* 1 max speed, no lazy matches */
      new Config(4, 5, 16, 8, deflate_fast),           /* 2 */
      new Config(4, 6, 32, 32, deflate_fast),          /* 3 */

      new Config(4, 4, 16, 16, deflate_slow),          /* 4 lazy matches */
      new Config(8, 16, 32, 32, deflate_slow),         /* 5 */
      new Config(8, 16, 128, 128, deflate_slow),       /* 6 */
      new Config(8, 32, 128, 256, deflate_slow),       /* 7 */
      new Config(32, 128, 258, 1024, deflate_slow),    /* 8 */
      new Config(32, 258, 258, 4096, deflate_slow)     /* 9 max compression */
    ];


    /* ===========================================================================
     * Initialize the "longest match" routines for a new zlib stream
     */
    const lm_init = (s) => {

      s.window_size = 2 * s.w_size;

      /*** CLEAR_HASH(s); ***/
      zero(s.head); // Fill with NIL (= 0);

      /* Set the default configuration parameters:
       */
      s.max_lazy_match = configuration_table[s.level].max_lazy;
      s.good_match = configuration_table[s.level].good_length;
      s.nice_match = configuration_table[s.level].nice_length;
      s.max_chain_length = configuration_table[s.level].max_chain;

      s.strstart = 0;
      s.block_start = 0;
      s.lookahead = 0;
      s.insert = 0;
      s.match_length = s.prev_length = MIN_MATCH - 1;
      s.match_available = 0;
      s.ins_h = 0;
    };


    function DeflateState() {
      this.strm = null;            /* pointer back to this zlib stream */
      this.status = 0;            /* as the name implies */
      this.pending_buf = null;      /* output still pending */
      this.pending_buf_size = 0;  /* size of pending_buf */
      this.pending_out = 0;       /* next pending byte to output to the stream */
      this.pending = 0;           /* nb of bytes in the pending buffer */
      this.wrap = 0;              /* bit 0 true for zlib, bit 1 true for gzip */
      this.gzhead = null;         /* gzip header information to write */
      this.gzindex = 0;           /* where in extra, name, or comment */
      this.method = Z_DEFLATED$2; /* can only be DEFLATED */
      this.last_flush = -1;   /* value of flush param for previous deflate call */

      this.w_size = 0;  /* LZ77 window size (32K by default) */
      this.w_bits = 0;  /* log2(w_size)  (8..16) */
      this.w_mask = 0;  /* w_size - 1 */

      this.window = null;
      /* Sliding window. Input bytes are read into the second half of the window,
       * and move to the first half later to keep a dictionary of at least wSize
       * bytes. With this organization, matches are limited to a distance of
       * wSize-MAX_MATCH bytes, but this ensures that IO is always
       * performed with a length multiple of the block size.
       */

      this.window_size = 0;
      /* Actual size of window: 2*wSize, except when the user input buffer
       * is directly used as sliding window.
       */

      this.prev = null;
      /* Link to older string with same hash index. To limit the size of this
       * array to 64K, this link is maintained only for the last 32K strings.
       * An index in this array is thus a window index modulo 32K.
       */

      this.head = null;   /* Heads of the hash chains or NIL. */

      this.ins_h = 0;       /* hash index of string to be inserted */
      this.hash_size = 0;   /* number of elements in hash table */
      this.hash_bits = 0;   /* log2(hash_size) */
      this.hash_mask = 0;   /* hash_size-1 */

      this.hash_shift = 0;
      /* Number of bits by which ins_h must be shifted at each input
       * step. It must be such that after MIN_MATCH steps, the oldest
       * byte no longer takes part in the hash key, that is:
       *   hash_shift * MIN_MATCH >= hash_bits
       */

      this.block_start = 0;
      /* Window position at the beginning of the current output block. Gets
       * negative when the window is moved backwards.
       */

      this.match_length = 0;      /* length of best match */
      this.prev_match = 0;        /* previous match */
      this.match_available = 0;   /* set if previous match exists */
      this.strstart = 0;          /* start of string to insert */
      this.match_start = 0;       /* start of matching string */
      this.lookahead = 0;         /* number of valid bytes ahead in window */

      this.prev_length = 0;
      /* Length of the best match at previous step. Matches not greater than this
       * are discarded. This is used in the lazy match evaluation.
       */

      this.max_chain_length = 0;
      /* To speed up deflation, hash chains are never searched beyond this
       * length.  A higher limit improves compression ratio but degrades the
       * speed.
       */

      this.max_lazy_match = 0;
      /* Attempt to find a better match only when the current match is strictly
       * smaller than this value. This mechanism is used only for compression
       * levels >= 4.
       */
      // That's alias to max_lazy_match, don't use directly
      //this.max_insert_length = 0;
      /* Insert new strings in the hash table only if the match length is not
       * greater than this length. This saves time but degrades compression.
       * max_insert_length is used only for compression levels <= 3.
       */

      this.level = 0;     /* compression level (1..9) */
      this.strategy = 0;  /* favor or force Huffman coding*/

      this.good_match = 0;
      /* Use a faster search when the previous match is longer than this */

      this.nice_match = 0; /* Stop searching when current match exceeds this */

                  /* used by trees.c: */

      /* Didn't use ct_data typedef below to suppress compiler warning */

      // struct ct_data_s dyn_ltree[HEAP_SIZE];   /* literal and length tree */
      // struct ct_data_s dyn_dtree[2*D_CODES+1]; /* distance tree */
      // struct ct_data_s bl_tree[2*BL_CODES+1];  /* Huffman tree for bit lengths */

      // Use flat array of DOUBLE size, with interleaved fata,
      // because JS does not support effective
      this.dyn_ltree  = new Uint16Array(HEAP_SIZE * 2);
      this.dyn_dtree  = new Uint16Array((2 * D_CODES + 1) * 2);
      this.bl_tree    = new Uint16Array((2 * BL_CODES + 1) * 2);
      zero(this.dyn_ltree);
      zero(this.dyn_dtree);
      zero(this.bl_tree);

      this.l_desc   = null;         /* desc. for literal tree */
      this.d_desc   = null;         /* desc. for distance tree */
      this.bl_desc  = null;         /* desc. for bit length tree */

      //ush bl_count[MAX_BITS+1];
      this.bl_count = new Uint16Array(MAX_BITS + 1);
      /* number of codes at each bit length for an optimal tree */

      //int heap[2*L_CODES+1];      /* heap used to build the Huffman trees */
      this.heap = new Uint16Array(2 * L_CODES + 1);  /* heap used to build the Huffman trees */
      zero(this.heap);

      this.heap_len = 0;               /* number of elements in the heap */
      this.heap_max = 0;               /* element of largest frequency */
      /* The sons of heap[n] are heap[2*n] and heap[2*n+1]. heap[0] is not used.
       * The same heap array is used to build all trees.
       */

      this.depth = new Uint16Array(2 * L_CODES + 1); //uch depth[2*L_CODES+1];
      zero(this.depth);
      /* Depth of each subtree used as tie breaker for trees of equal frequency
       */

      this.sym_buf = 0;        /* buffer for distances and literals/lengths */

      this.lit_bufsize = 0;
      /* Size of match buffer for literals/lengths.  There are 4 reasons for
       * limiting lit_bufsize to 64K:
       *   - frequencies can be kept in 16 bit counters
       *   - if compression is not successful for the first block, all input
       *     data is still in the window so we can still emit a stored block even
       *     when input comes from standard input.  (This can also be done for
       *     all blocks if lit_bufsize is not greater than 32K.)
       *   - if compression is not successful for a file smaller than 64K, we can
       *     even emit a stored file instead of a stored block (saving 5 bytes).
       *     This is applicable only for zip (not gzip or zlib).
       *   - creating new Huffman trees less frequently may not provide fast
       *     adaptation to changes in the input data statistics. (Take for
       *     example a binary file with poorly compressible code followed by
       *     a highly compressible string table.) Smaller buffer sizes give
       *     fast adaptation but have of course the overhead of transmitting
       *     trees more frequently.
       *   - I can't count above 4
       */

      this.sym_next = 0;      /* running index in sym_buf */
      this.sym_end = 0;       /* symbol table full when sym_next reaches this */

      this.opt_len = 0;       /* bit length of current block with optimal trees */
      this.static_len = 0;    /* bit length of current block with static trees */
      this.matches = 0;       /* number of string matches in current block */
      this.insert = 0;        /* bytes at end of window left to insert */


      this.bi_buf = 0;
      /* Output buffer. bits are inserted starting at the bottom (least
       * significant bits).
       */
      this.bi_valid = 0;
      /* Number of valid bits in bi_buf.  All bits above the last valid bit
       * are always zero.
       */

      // Used for window memory init. We safely ignore it for JS. That makes
      // sense only for pointers and memory check tools.
      //this.high_water = 0;
      /* High water mark offset in window for initialized bytes -- bytes above
       * this are set to zero in order to avoid memory check warnings when
       * longest match routines access bytes past the input.  This is then
       * updated to the new high water mark.
       */
    }


    /* =========================================================================
     * Check for a valid deflate stream state. Return 0 if ok, 1 if not.
     */
    const deflateStateCheck = (strm) => {

      if (!strm) {
        return 1;
      }
      const s = strm.state;
      if (!s || s.strm !== strm || (s.status !== INIT_STATE &&
    //#ifdef GZIP
                                    s.status !== GZIP_STATE &&
    //#endif
                                    s.status !== EXTRA_STATE &&
                                    s.status !== NAME_STATE &&
                                    s.status !== COMMENT_STATE &&
                                    s.status !== HCRC_STATE &&
                                    s.status !== BUSY_STATE &&
                                    s.status !== FINISH_STATE)) {
        return 1;
      }
      return 0;
    };


    const deflateResetKeep = (strm) => {

      if (deflateStateCheck(strm)) {
        return err(strm, Z_STREAM_ERROR$2);
      }

      strm.total_in = strm.total_out = 0;
      strm.data_type = Z_UNKNOWN;

      const s = strm.state;
      s.pending = 0;
      s.pending_out = 0;

      if (s.wrap < 0) {
        s.wrap = -s.wrap;
        /* was made negative by deflate(..., Z_FINISH); */
      }
      s.status =
    //#ifdef GZIP
        s.wrap === 2 ? GZIP_STATE :
    //#endif
        s.wrap ? INIT_STATE : BUSY_STATE;
      strm.adler = (s.wrap === 2) ?
        0  // crc32(0, Z_NULL, 0)
      :
        1; // adler32(0, Z_NULL, 0)
      s.last_flush = -2;
      _tr_init(s);
      return Z_OK$3;
    };


    const deflateReset = (strm) => {

      const ret = deflateResetKeep(strm);
      if (ret === Z_OK$3) {
        lm_init(strm.state);
      }
      return ret;
    };


    const deflateSetHeader = (strm, head) => {

      if (deflateStateCheck(strm) || strm.state.wrap !== 2) {
        return Z_STREAM_ERROR$2;
      }
      strm.state.gzhead = head;
      return Z_OK$3;
    };


    const deflateInit2 = (strm, level, method, windowBits, memLevel, strategy) => {

      if (!strm) { // === Z_NULL
        return Z_STREAM_ERROR$2;
      }
      let wrap = 1;

      if (level === Z_DEFAULT_COMPRESSION$1) {
        level = 6;
      }

      if (windowBits < 0) { /* suppress zlib wrapper */
        wrap = 0;
        windowBits = -windowBits;
      }

      else if (windowBits > 15) {
        wrap = 2;           /* write gzip wrapper instead */
        windowBits -= 16;
      }


      if (memLevel < 1 || memLevel > MAX_MEM_LEVEL || method !== Z_DEFLATED$2 ||
        windowBits < 8 || windowBits > 15 || level < 0 || level > 9 ||
        strategy < 0 || strategy > Z_FIXED || (windowBits === 8 && wrap !== 1)) {
        return err(strm, Z_STREAM_ERROR$2);
      }


      if (windowBits === 8) {
        windowBits = 9;
      }
      /* until 256-byte window bug fixed */

      const s = new DeflateState();

      strm.state = s;
      s.strm = strm;
      s.status = INIT_STATE;     /* to pass state test in deflateReset() */

      s.wrap = wrap;
      s.gzhead = null;
      s.w_bits = windowBits;
      s.w_size = 1 << s.w_bits;
      s.w_mask = s.w_size - 1;

      s.hash_bits = memLevel + 7;
      s.hash_size = 1 << s.hash_bits;
      s.hash_mask = s.hash_size - 1;
      s.hash_shift = ~~((s.hash_bits + MIN_MATCH - 1) / MIN_MATCH);

      s.window = new Uint8Array(s.w_size * 2);
      s.head = new Uint16Array(s.hash_size);
      s.prev = new Uint16Array(s.w_size);

      // Don't need mem init magic for JS.
      //s.high_water = 0;  /* nothing written to s->window yet */

      s.lit_bufsize = 1 << (memLevel + 6); /* 16K elements by default */

      /* We overlay pending_buf and sym_buf. This works since the average size
       * for length/distance pairs over any compressed block is assured to be 31
       * bits or less.
       *
       * Analysis: The longest fixed codes are a length code of 8 bits plus 5
       * extra bits, for lengths 131 to 257. The longest fixed distance codes are
       * 5 bits plus 13 extra bits, for distances 16385 to 32768. The longest
       * possible fixed-codes length/distance pair is then 31 bits total.
       *
       * sym_buf starts one-fourth of the way into pending_buf. So there are
       * three bytes in sym_buf for every four bytes in pending_buf. Each symbol
       * in sym_buf is three bytes -- two for the distance and one for the
       * literal/length. As each symbol is consumed, the pointer to the next
       * sym_buf value to read moves forward three bytes. From that symbol, up to
       * 31 bits are written to pending_buf. The closest the written pending_buf
       * bits gets to the next sym_buf symbol to read is just before the last
       * code is written. At that time, 31*(n-2) bits have been written, just
       * after 24*(n-2) bits have been consumed from sym_buf. sym_buf starts at
       * 8*n bits into pending_buf. (Note that the symbol buffer fills when n-1
       * symbols are written.) The closest the writing gets to what is unread is
       * then n+14 bits. Here n is lit_bufsize, which is 16384 by default, and
       * can range from 128 to 32768.
       *
       * Therefore, at a minimum, there are 142 bits of space between what is
       * written and what is read in the overlain buffers, so the symbols cannot
       * be overwritten by the compressed data. That space is actually 139 bits,
       * due to the three-bit fixed-code block header.
       *
       * That covers the case where either Z_FIXED is specified, forcing fixed
       * codes, or when the use of fixed codes is chosen, because that choice
       * results in a smaller compressed block than dynamic codes. That latter
       * condition then assures that the above analysis also covers all dynamic
       * blocks. A dynamic-code block will only be chosen to be emitted if it has
       * fewer bits than a fixed-code block would for the same set of symbols.
       * Therefore its average symbol length is assured to be less than 31. So
       * the compressed data for a dynamic block also cannot overwrite the
       * symbols from which it is being constructed.
       */

      s.pending_buf_size = s.lit_bufsize * 4;
      s.pending_buf = new Uint8Array(s.pending_buf_size);

      // It is offset from `s.pending_buf` (size is `s.lit_bufsize * 2`)
      //s->sym_buf = s->pending_buf + s->lit_bufsize;
      s.sym_buf = s.lit_bufsize;

      //s->sym_end = (s->lit_bufsize - 1) * 3;
      s.sym_end = (s.lit_bufsize - 1) * 3;
      /* We avoid equality with lit_bufsize*3 because of wraparound at 64K
       * on 16 bit machines and because stored blocks are restricted to
       * 64K-1 bytes.
       */

      s.level = level;
      s.strategy = strategy;
      s.method = method;

      return deflateReset(strm);
    };

    const deflateInit = (strm, level) => {

      return deflateInit2(strm, level, Z_DEFLATED$2, MAX_WBITS$1, DEF_MEM_LEVEL, Z_DEFAULT_STRATEGY$1);
    };


    /* ========================================================================= */
    const deflate$2 = (strm, flush) => {

      if (deflateStateCheck(strm) || flush > Z_BLOCK$1 || flush < 0) {
        return strm ? err(strm, Z_STREAM_ERROR$2) : Z_STREAM_ERROR$2;
      }

      const s = strm.state;

      if (!strm.output ||
          (strm.avail_in !== 0 && !strm.input) ||
          (s.status === FINISH_STATE && flush !== Z_FINISH$3)) {
        return err(strm, (strm.avail_out === 0) ? Z_BUF_ERROR$1 : Z_STREAM_ERROR$2);
      }

      const old_flush = s.last_flush;
      s.last_flush = flush;

      /* Flush as much pending output as possible */
      if (s.pending !== 0) {
        flush_pending(strm);
        if (strm.avail_out === 0) {
          /* Since avail_out is 0, deflate will be called again with
           * more output space, but possibly with both pending and
           * avail_in equal to zero. There won't be anything to do,
           * but this is not an error situation so make sure we
           * return OK instead of BUF_ERROR at next call of deflate:
           */
          s.last_flush = -1;
          return Z_OK$3;
        }

        /* Make sure there is something to do and avoid duplicate consecutive
         * flushes. For repeated and useless calls with Z_FINISH, we keep
         * returning Z_STREAM_END instead of Z_BUF_ERROR.
         */
      } else if (strm.avail_in === 0 && rank(flush) <= rank(old_flush) &&
        flush !== Z_FINISH$3) {
        return err(strm, Z_BUF_ERROR$1);
      }

      /* User must not provide more input after the first FINISH: */
      if (s.status === FINISH_STATE && strm.avail_in !== 0) {
        return err(strm, Z_BUF_ERROR$1);
      }

      /* Write the header */
      if (s.status === INIT_STATE && s.wrap === 0) {
        s.status = BUSY_STATE;
      }
      if (s.status === INIT_STATE) {
        /* zlib header */
        let header = (Z_DEFLATED$2 + ((s.w_bits - 8) << 4)) << 8;
        let level_flags = -1;

        if (s.strategy >= Z_HUFFMAN_ONLY || s.level < 2) {
          level_flags = 0;
        } else if (s.level < 6) {
          level_flags = 1;
        } else if (s.level === 6) {
          level_flags = 2;
        } else {
          level_flags = 3;
        }
        header |= (level_flags << 6);
        if (s.strstart !== 0) { header |= PRESET_DICT; }
        header += 31 - (header % 31);

        putShortMSB(s, header);

        /* Save the adler32 of the preset dictionary: */
        if (s.strstart !== 0) {
          putShortMSB(s, strm.adler >>> 16);
          putShortMSB(s, strm.adler & 0xffff);
        }
        strm.adler = 1; // adler32(0L, Z_NULL, 0);
        s.status = BUSY_STATE;

        /* Compression must start with an empty pending buffer */
        flush_pending(strm);
        if (s.pending !== 0) {
          s.last_flush = -1;
          return Z_OK$3;
        }
      }
    //#ifdef GZIP
      if (s.status === GZIP_STATE) {
        /* gzip header */
        strm.adler = 0;  //crc32(0L, Z_NULL, 0);
        put_byte(s, 31);
        put_byte(s, 139);
        put_byte(s, 8);
        if (!s.gzhead) { // s->gzhead == Z_NULL
          put_byte(s, 0);
          put_byte(s, 0);
          put_byte(s, 0);
          put_byte(s, 0);
          put_byte(s, 0);
          put_byte(s, s.level === 9 ? 2 :
                      (s.strategy >= Z_HUFFMAN_ONLY || s.level < 2 ?
                       4 : 0));
          put_byte(s, OS_CODE);
          s.status = BUSY_STATE;

          /* Compression must start with an empty pending buffer */
          flush_pending(strm);
          if (s.pending !== 0) {
            s.last_flush = -1;
            return Z_OK$3;
          }
        }
        else {
          put_byte(s, (s.gzhead.text ? 1 : 0) +
                      (s.gzhead.hcrc ? 2 : 0) +
                      (!s.gzhead.extra ? 0 : 4) +
                      (!s.gzhead.name ? 0 : 8) +
                      (!s.gzhead.comment ? 0 : 16)
          );
          put_byte(s, s.gzhead.time & 0xff);
          put_byte(s, (s.gzhead.time >> 8) & 0xff);
          put_byte(s, (s.gzhead.time >> 16) & 0xff);
          put_byte(s, (s.gzhead.time >> 24) & 0xff);
          put_byte(s, s.level === 9 ? 2 :
                      (s.strategy >= Z_HUFFMAN_ONLY || s.level < 2 ?
                       4 : 0));
          put_byte(s, s.gzhead.os & 0xff);
          if (s.gzhead.extra && s.gzhead.extra.length) {
            put_byte(s, s.gzhead.extra.length & 0xff);
            put_byte(s, (s.gzhead.extra.length >> 8) & 0xff);
          }
          if (s.gzhead.hcrc) {
            strm.adler = crc32_1(strm.adler, s.pending_buf, s.pending, 0);
          }
          s.gzindex = 0;
          s.status = EXTRA_STATE;
        }
      }
      if (s.status === EXTRA_STATE) {
        if (s.gzhead.extra/* != Z_NULL*/) {
          let beg = s.pending;   /* start of bytes to update crc */
          let left = (s.gzhead.extra.length & 0xffff) - s.gzindex;
          while (s.pending + left > s.pending_buf_size) {
            let copy = s.pending_buf_size - s.pending;
            // zmemcpy(s.pending_buf + s.pending,
            //    s.gzhead.extra + s.gzindex, copy);
            s.pending_buf.set(s.gzhead.extra.subarray(s.gzindex, s.gzindex + copy), s.pending);
            s.pending = s.pending_buf_size;
            //--- HCRC_UPDATE(beg) ---//
            if (s.gzhead.hcrc && s.pending > beg) {
              strm.adler = crc32_1(strm.adler, s.pending_buf, s.pending - beg, beg);
            }
            //---//
            s.gzindex += copy;
            flush_pending(strm);
            if (s.pending !== 0) {
              s.last_flush = -1;
              return Z_OK$3;
            }
            beg = 0;
            left -= copy;
          }
          // JS specific: s.gzhead.extra may be TypedArray or Array for backward compatibility
          //              TypedArray.slice and TypedArray.from don't exist in IE10-IE11
          let gzhead_extra = new Uint8Array(s.gzhead.extra);
          // zmemcpy(s->pending_buf + s->pending,
          //     s->gzhead->extra + s->gzindex, left);
          s.pending_buf.set(gzhead_extra.subarray(s.gzindex, s.gzindex + left), s.pending);
          s.pending += left;
          //--- HCRC_UPDATE(beg) ---//
          if (s.gzhead.hcrc && s.pending > beg) {
            strm.adler = crc32_1(strm.adler, s.pending_buf, s.pending - beg, beg);
          }
          //---//
          s.gzindex = 0;
        }
        s.status = NAME_STATE;
      }
      if (s.status === NAME_STATE) {
        if (s.gzhead.name/* != Z_NULL*/) {
          let beg = s.pending;   /* start of bytes to update crc */
          let val;
          do {
            if (s.pending === s.pending_buf_size) {
              //--- HCRC_UPDATE(beg) ---//
              if (s.gzhead.hcrc && s.pending > beg) {
                strm.adler = crc32_1(strm.adler, s.pending_buf, s.pending - beg, beg);
              }
              //---//
              flush_pending(strm);
              if (s.pending !== 0) {
                s.last_flush = -1;
                return Z_OK$3;
              }
              beg = 0;
            }
            // JS specific: little magic to add zero terminator to end of string
            if (s.gzindex < s.gzhead.name.length) {
              val = s.gzhead.name.charCodeAt(s.gzindex++) & 0xff;
            } else {
              val = 0;
            }
            put_byte(s, val);
          } while (val !== 0);
          //--- HCRC_UPDATE(beg) ---//
          if (s.gzhead.hcrc && s.pending > beg) {
            strm.adler = crc32_1(strm.adler, s.pending_buf, s.pending - beg, beg);
          }
          //---//
          s.gzindex = 0;
        }
        s.status = COMMENT_STATE;
      }
      if (s.status === COMMENT_STATE) {
        if (s.gzhead.comment/* != Z_NULL*/) {
          let beg = s.pending;   /* start of bytes to update crc */
          let val;
          do {
            if (s.pending === s.pending_buf_size) {
              //--- HCRC_UPDATE(beg) ---//
              if (s.gzhead.hcrc && s.pending > beg) {
                strm.adler = crc32_1(strm.adler, s.pending_buf, s.pending - beg, beg);
              }
              //---//
              flush_pending(strm);
              if (s.pending !== 0) {
                s.last_flush = -1;
                return Z_OK$3;
              }
              beg = 0;
            }
            // JS specific: little magic to add zero terminator to end of string
            if (s.gzindex < s.gzhead.comment.length) {
              val = s.gzhead.comment.charCodeAt(s.gzindex++) & 0xff;
            } else {
              val = 0;
            }
            put_byte(s, val);
          } while (val !== 0);
          //--- HCRC_UPDATE(beg) ---//
          if (s.gzhead.hcrc && s.pending > beg) {
            strm.adler = crc32_1(strm.adler, s.pending_buf, s.pending - beg, beg);
          }
          //---//
        }
        s.status = HCRC_STATE;
      }
      if (s.status === HCRC_STATE) {
        if (s.gzhead.hcrc) {
          if (s.pending + 2 > s.pending_buf_size) {
            flush_pending(strm);
            if (s.pending !== 0) {
              s.last_flush = -1;
              return Z_OK$3;
            }
          }
          put_byte(s, strm.adler & 0xff);
          put_byte(s, (strm.adler >> 8) & 0xff);
          strm.adler = 0; //crc32(0L, Z_NULL, 0);
        }
        s.status = BUSY_STATE;

        /* Compression must start with an empty pending buffer */
        flush_pending(strm);
        if (s.pending !== 0) {
          s.last_flush = -1;
          return Z_OK$3;
        }
      }
    //#endif

      /* Start a new block or continue the current one.
       */
      if (strm.avail_in !== 0 || s.lookahead !== 0 ||
        (flush !== Z_NO_FLUSH$2 && s.status !== FINISH_STATE)) {
        let bstate = s.level === 0 ? deflate_stored(s, flush) :
                     s.strategy === Z_HUFFMAN_ONLY ? deflate_huff(s, flush) :
                     s.strategy === Z_RLE ? deflate_rle(s, flush) :
                     configuration_table[s.level].func(s, flush);

        if (bstate === BS_FINISH_STARTED || bstate === BS_FINISH_DONE) {
          s.status = FINISH_STATE;
        }
        if (bstate === BS_NEED_MORE || bstate === BS_FINISH_STARTED) {
          if (strm.avail_out === 0) {
            s.last_flush = -1;
            /* avoid BUF_ERROR next call, see above */
          }
          return Z_OK$3;
          /* If flush != Z_NO_FLUSH && avail_out == 0, the next call
           * of deflate should use the same flush parameter to make sure
           * that the flush is complete. So we don't have to output an
           * empty block here, this will be done at next call. This also
           * ensures that for a very small output buffer, we emit at most
           * one empty block.
           */
        }
        if (bstate === BS_BLOCK_DONE) {
          if (flush === Z_PARTIAL_FLUSH) {
            _tr_align(s);
          }
          else if (flush !== Z_BLOCK$1) { /* FULL_FLUSH or SYNC_FLUSH */

            _tr_stored_block(s, 0, 0, false);
            /* For a full flush, this empty block will be recognized
             * as a special marker by inflate_sync().
             */
            if (flush === Z_FULL_FLUSH$1) {
              /*** CLEAR_HASH(s); ***/             /* forget history */
              zero(s.head); // Fill with NIL (= 0);

              if (s.lookahead === 0) {
                s.strstart = 0;
                s.block_start = 0;
                s.insert = 0;
              }
            }
          }
          flush_pending(strm);
          if (strm.avail_out === 0) {
            s.last_flush = -1; /* avoid BUF_ERROR at next call, see above */
            return Z_OK$3;
          }
        }
      }

      if (flush !== Z_FINISH$3) { return Z_OK$3; }
      if (s.wrap <= 0) { return Z_STREAM_END$3; }

      /* Write the trailer */
      if (s.wrap === 2) {
        put_byte(s, strm.adler & 0xff);
        put_byte(s, (strm.adler >> 8) & 0xff);
        put_byte(s, (strm.adler >> 16) & 0xff);
        put_byte(s, (strm.adler >> 24) & 0xff);
        put_byte(s, strm.total_in & 0xff);
        put_byte(s, (strm.total_in >> 8) & 0xff);
        put_byte(s, (strm.total_in >> 16) & 0xff);
        put_byte(s, (strm.total_in >> 24) & 0xff);
      }
      else
      {
        putShortMSB(s, strm.adler >>> 16);
        putShortMSB(s, strm.adler & 0xffff);
      }

      flush_pending(strm);
      /* If avail_out is zero, the application will call deflate again
       * to flush the rest.
       */
      if (s.wrap > 0) { s.wrap = -s.wrap; }
      /* write the trailer only once! */
      return s.pending !== 0 ? Z_OK$3 : Z_STREAM_END$3;
    };


    const deflateEnd = (strm) => {

      if (deflateStateCheck(strm)) {
        return Z_STREAM_ERROR$2;
      }

      const status = strm.state.status;

      strm.state = null;

      return status === BUSY_STATE ? err(strm, Z_DATA_ERROR$2) : Z_OK$3;
    };


    /* =========================================================================
     * Initializes the compression dictionary from the given byte
     * sequence without producing any compressed output.
     */
    const deflateSetDictionary = (strm, dictionary) => {

      let dictLength = dictionary.length;

      if (deflateStateCheck(strm)) {
        return Z_STREAM_ERROR$2;
      }

      const s = strm.state;
      const wrap = s.wrap;

      if (wrap === 2 || (wrap === 1 && s.status !== INIT_STATE) || s.lookahead) {
        return Z_STREAM_ERROR$2;
      }

      /* when using zlib wrappers, compute Adler-32 for provided dictionary */
      if (wrap === 1) {
        /* adler32(strm->adler, dictionary, dictLength); */
        strm.adler = adler32_1(strm.adler, dictionary, dictLength, 0);
      }

      s.wrap = 0;   /* avoid computing Adler-32 in read_buf */

      /* if dictionary would fill window, just replace the history */
      if (dictLength >= s.w_size) {
        if (wrap === 0) {            /* already empty otherwise */
          /*** CLEAR_HASH(s); ***/
          zero(s.head); // Fill with NIL (= 0);
          s.strstart = 0;
          s.block_start = 0;
          s.insert = 0;
        }
        /* use the tail */
        // dictionary = dictionary.slice(dictLength - s.w_size);
        let tmpDict = new Uint8Array(s.w_size);
        tmpDict.set(dictionary.subarray(dictLength - s.w_size, dictLength), 0);
        dictionary = tmpDict;
        dictLength = s.w_size;
      }
      /* insert dictionary into window and hash */
      const avail = strm.avail_in;
      const next = strm.next_in;
      const input = strm.input;
      strm.avail_in = dictLength;
      strm.next_in = 0;
      strm.input = dictionary;
      fill_window(s);
      while (s.lookahead >= MIN_MATCH) {
        let str = s.strstart;
        let n = s.lookahead - (MIN_MATCH - 1);
        do {
          /* UPDATE_HASH(s, s->ins_h, s->window[str + MIN_MATCH-1]); */
          s.ins_h = HASH(s, s.ins_h, s.window[str + MIN_MATCH - 1]);

          s.prev[str & s.w_mask] = s.head[s.ins_h];

          s.head[s.ins_h] = str;
          str++;
        } while (--n);
        s.strstart = str;
        s.lookahead = MIN_MATCH - 1;
        fill_window(s);
      }
      s.strstart += s.lookahead;
      s.block_start = s.strstart;
      s.insert = s.lookahead;
      s.lookahead = 0;
      s.match_length = s.prev_length = MIN_MATCH - 1;
      s.match_available = 0;
      strm.next_in = next;
      strm.input = input;
      strm.avail_in = avail;
      s.wrap = wrap;
      return Z_OK$3;
    };


    var deflateInit_1 = deflateInit;
    var deflateInit2_1 = deflateInit2;
    var deflateReset_1 = deflateReset;
    var deflateResetKeep_1 = deflateResetKeep;
    var deflateSetHeader_1 = deflateSetHeader;
    var deflate_2$1 = deflate$2;
    var deflateEnd_1 = deflateEnd;
    var deflateSetDictionary_1 = deflateSetDictionary;
    var deflateInfo = 'pako deflate (from Nodeca project)';

    /* Not implemented
    module.exports.deflateBound = deflateBound;
    module.exports.deflateCopy = deflateCopy;
    module.exports.deflateGetDictionary = deflateGetDictionary;
    module.exports.deflateParams = deflateParams;
    module.exports.deflatePending = deflatePending;
    module.exports.deflatePrime = deflatePrime;
    module.exports.deflateTune = deflateTune;
    */

    var deflate_1$2 = {
    	deflateInit: deflateInit_1,
    	deflateInit2: deflateInit2_1,
    	deflateReset: deflateReset_1,
    	deflateResetKeep: deflateResetKeep_1,
    	deflateSetHeader: deflateSetHeader_1,
    	deflate: deflate_2$1,
    	deflateEnd: deflateEnd_1,
    	deflateSetDictionary: deflateSetDictionary_1,
    	deflateInfo: deflateInfo
    };

    const _has = (obj, key) => {
      return Object.prototype.hasOwnProperty.call(obj, key);
    };

    var assign = function (obj /*from1, from2, from3, ...*/) {
      const sources = Array.prototype.slice.call(arguments, 1);
      while (sources.length) {
        const source = sources.shift();
        if (!source) { continue; }

        if (typeof source !== 'object') {
          throw new TypeError(source + 'must be non-object');
        }

        for (const p in source) {
          if (_has(source, p)) {
            obj[p] = source[p];
          }
        }
      }

      return obj;
    };


    // Join array of chunks to single array.
    var flattenChunks = (chunks) => {
      // calculate data length
      let len = 0;

      for (let i = 0, l = chunks.length; i < l; i++) {
        len += chunks[i].length;
      }

      // join chunks
      const result = new Uint8Array(len);

      for (let i = 0, pos = 0, l = chunks.length; i < l; i++) {
        let chunk = chunks[i];
        result.set(chunk, pos);
        pos += chunk.length;
      }

      return result;
    };

    var common = {
    	assign: assign,
    	flattenChunks: flattenChunks
    };

    // String encode/decode helpers


    // Quick check if we can use fast array to bin string conversion
    //
    // - apply(Array) can fail on Android 2.2
    // - apply(Uint8Array) can fail on iOS 5.1 Safari
    //
    let STR_APPLY_UIA_OK = true;

    try { String.fromCharCode.apply(null, new Uint8Array(1)); } catch (__) { STR_APPLY_UIA_OK = false; }


    // Table with utf8 lengths (calculated by first byte of sequence)
    // Note, that 5 & 6-byte values and some 4-byte values can not be represented in JS,
    // because max possible codepoint is 0x10ffff
    const _utf8len = new Uint8Array(256);
    for (let q = 0; q < 256; q++) {
      _utf8len[q] = (q >= 252 ? 6 : q >= 248 ? 5 : q >= 240 ? 4 : q >= 224 ? 3 : q >= 192 ? 2 : 1);
    }
    _utf8len[254] = _utf8len[254] = 1; // Invalid sequence start


    // convert string to array (typed, when possible)
    var string2buf = (str) => {
      if (typeof TextEncoder === 'function' && TextEncoder.prototype.encode) {
        return new TextEncoder().encode(str);
      }

      let buf, c, c2, m_pos, i, str_len = str.length, buf_len = 0;

      // count binary size
      for (m_pos = 0; m_pos < str_len; m_pos++) {
        c = str.charCodeAt(m_pos);
        if ((c & 0xfc00) === 0xd800 && (m_pos + 1 < str_len)) {
          c2 = str.charCodeAt(m_pos + 1);
          if ((c2 & 0xfc00) === 0xdc00) {
            c = 0x10000 + ((c - 0xd800) << 10) + (c2 - 0xdc00);
            m_pos++;
          }
        }
        buf_len += c < 0x80 ? 1 : c < 0x800 ? 2 : c < 0x10000 ? 3 : 4;
      }

      // allocate buffer
      buf = new Uint8Array(buf_len);

      // convert
      for (i = 0, m_pos = 0; i < buf_len; m_pos++) {
        c = str.charCodeAt(m_pos);
        if ((c & 0xfc00) === 0xd800 && (m_pos + 1 < str_len)) {
          c2 = str.charCodeAt(m_pos + 1);
          if ((c2 & 0xfc00) === 0xdc00) {
            c = 0x10000 + ((c - 0xd800) << 10) + (c2 - 0xdc00);
            m_pos++;
          }
        }
        if (c < 0x80) {
          /* one byte */
          buf[i++] = c;
        } else if (c < 0x800) {
          /* two bytes */
          buf[i++] = 0xC0 | (c >>> 6);
          buf[i++] = 0x80 | (c & 0x3f);
        } else if (c < 0x10000) {
          /* three bytes */
          buf[i++] = 0xE0 | (c >>> 12);
          buf[i++] = 0x80 | (c >>> 6 & 0x3f);
          buf[i++] = 0x80 | (c & 0x3f);
        } else {
          /* four bytes */
          buf[i++] = 0xf0 | (c >>> 18);
          buf[i++] = 0x80 | (c >>> 12 & 0x3f);
          buf[i++] = 0x80 | (c >>> 6 & 0x3f);
          buf[i++] = 0x80 | (c & 0x3f);
        }
      }

      return buf;
    };

    // Helper
    const buf2binstring = (buf, len) => {
      // On Chrome, the arguments in a function call that are allowed is `65534`.
      // If the length of the buffer is smaller than that, we can use this optimization,
      // otherwise we will take a slower path.
      if (len < 65534) {
        if (buf.subarray && STR_APPLY_UIA_OK) {
          return String.fromCharCode.apply(null, buf.length === len ? buf : buf.subarray(0, len));
        }
      }

      let result = '';
      for (let i = 0; i < len; i++) {
        result += String.fromCharCode(buf[i]);
      }
      return result;
    };


    // convert array to string
    var buf2string = (buf, max) => {
      const len = max || buf.length;

      if (typeof TextDecoder === 'function' && TextDecoder.prototype.decode) {
        return new TextDecoder().decode(buf.subarray(0, max));
      }

      let i, out;

      // Reserve max possible length (2 words per char)
      // NB: by unknown reasons, Array is significantly faster for
      //     String.fromCharCode.apply than Uint16Array.
      const utf16buf = new Array(len * 2);

      for (out = 0, i = 0; i < len;) {
        let c = buf[i++];
        // quick process ascii
        if (c < 0x80) { utf16buf[out++] = c; continue; }

        let c_len = _utf8len[c];
        // skip 5 & 6 byte codes
        if (c_len > 4) { utf16buf[out++] = 0xfffd; i += c_len - 1; continue; }

        // apply mask on first byte
        c &= c_len === 2 ? 0x1f : c_len === 3 ? 0x0f : 0x07;
        // join the rest
        while (c_len > 1 && i < len) {
          c = (c << 6) | (buf[i++] & 0x3f);
          c_len--;
        }

        // terminated by end of string?
        if (c_len > 1) { utf16buf[out++] = 0xfffd; continue; }

        if (c < 0x10000) {
          utf16buf[out++] = c;
        } else {
          c -= 0x10000;
          utf16buf[out++] = 0xd800 | ((c >> 10) & 0x3ff);
          utf16buf[out++] = 0xdc00 | (c & 0x3ff);
        }
      }

      return buf2binstring(utf16buf, out);
    };


    // Calculate max possible position in utf8 buffer,
    // that will not break sequence. If that's not possible
    // - (very small limits) return max size as is.
    //
    // buf[] - utf8 bytes array
    // max   - length limit (mandatory);
    var utf8border = (buf, max) => {

      max = max || buf.length;
      if (max > buf.length) { max = buf.length; }

      // go back from last position, until start of sequence found
      let pos = max - 1;
      while (pos >= 0 && (buf[pos] & 0xC0) === 0x80) { pos--; }

      // Very small and broken sequence,
      // return max, because we should return something anyway.
      if (pos < 0) { return max; }

      // If we came to start of buffer - that means buffer is too small,
      // return max too.
      if (pos === 0) { return max; }

      return (pos + _utf8len[buf[pos]] > max) ? pos : max;
    };

    var strings = {
    	string2buf: string2buf,
    	buf2string: buf2string,
    	utf8border: utf8border
    };

    // (C) 1995-2013 Jean-loup Gailly and Mark Adler
    // (C) 2014-2017 Vitaly Puzrin and Andrey Tupitsin
    //
    // This software is provided 'as-is', without any express or implied
    // warranty. In no event will the authors be held liable for any damages
    // arising from the use of this software.
    //
    // Permission is granted to anyone to use this software for any purpose,
    // including commercial applications, and to alter it and redistribute it
    // freely, subject to the following restrictions:
    //
    // 1. The origin of this software must not be misrepresented; you must not
    //   claim that you wrote the original software. If you use this software
    //   in a product, an acknowledgment in the product documentation would be
    //   appreciated but is not required.
    // 2. Altered source versions must be plainly marked as such, and must not be
    //   misrepresented as being the original software.
    // 3. This notice may not be removed or altered from any source distribution.

    function ZStream() {
      /* next input byte */
      this.input = null; // JS specific, because we have no pointers
      this.next_in = 0;
      /* number of bytes available at input */
      this.avail_in = 0;
      /* total number of input bytes read so far */
      this.total_in = 0;
      /* next output byte should be put there */
      this.output = null; // JS specific, because we have no pointers
      this.next_out = 0;
      /* remaining free space at output */
      this.avail_out = 0;
      /* total number of bytes output so far */
      this.total_out = 0;
      /* last error message, NULL if no error */
      this.msg = ''/*Z_NULL*/;
      /* not visible by applications */
      this.state = null;
      /* best guess about the data type: binary or text */
      this.data_type = 2/*Z_UNKNOWN*/;
      /* adler32 value of the uncompressed data */
      this.adler = 0;
    }

    var zstream = ZStream;

    const toString$1 = Object.prototype.toString;

    /* Public constants ==========================================================*/
    /* ===========================================================================*/

    const {
      Z_NO_FLUSH: Z_NO_FLUSH$1, Z_SYNC_FLUSH, Z_FULL_FLUSH, Z_FINISH: Z_FINISH$2,
      Z_OK: Z_OK$2, Z_STREAM_END: Z_STREAM_END$2,
      Z_DEFAULT_COMPRESSION,
      Z_DEFAULT_STRATEGY,
      Z_DEFLATED: Z_DEFLATED$1
    } = constants$2;

    /* ===========================================================================*/


    /**
     * class Deflate
     *
     * Generic JS-style wrapper for zlib calls. If you don't need
     * streaming behaviour - use more simple functions: [[deflate]],
     * [[deflateRaw]] and [[gzip]].
     **/

    /* internal
     * Deflate.chunks -> Array
     *
     * Chunks of output data, if [[Deflate#onData]] not overridden.
     **/

    /**
     * Deflate.result -> Uint8Array
     *
     * Compressed result, generated by default [[Deflate#onData]]
     * and [[Deflate#onEnd]] handlers. Filled after you push last chunk
     * (call [[Deflate#push]] with `Z_FINISH` / `true` param).
     **/

    /**
     * Deflate.err -> Number
     *
     * Error code after deflate finished. 0 (Z_OK) on success.
     * You will not need it in real life, because deflate errors
     * are possible only on wrong options or bad `onData` / `onEnd`
     * custom handlers.
     **/

    /**
     * Deflate.msg -> String
     *
     * Error message, if [[Deflate.err]] != 0
     **/


    /**
     * new Deflate(options)
     * - options (Object): zlib deflate options.
     *
     * Creates new deflator instance with specified params. Throws exception
     * on bad params. Supported options:
     *
     * - `level`
     * - `windowBits`
     * - `memLevel`
     * - `strategy`
     * - `dictionary`
     *
     * [http://zlib.net/manual.html#Advanced](http://zlib.net/manual.html#Advanced)
     * for more information on these.
     *
     * Additional options, for internal needs:
     *
     * - `chunkSize` - size of generated data chunks (16K by default)
     * - `raw` (Boolean) - do raw deflate
     * - `gzip` (Boolean) - create gzip wrapper
     * - `header` (Object) - custom header for gzip
     *   - `text` (Boolean) - true if compressed data believed to be text
     *   - `time` (Number) - modification time, unix timestamp
     *   - `os` (Number) - operation system code
     *   - `extra` (Array) - array of bytes with extra data (max 65536)
     *   - `name` (String) - file name (binary string)
     *   - `comment` (String) - comment (binary string)
     *   - `hcrc` (Boolean) - true if header crc should be added
     *
     * ##### Example:
     *
     * ```javascript
     * const pako = require('pako')
     *   , chunk1 = new Uint8Array([1,2,3,4,5,6,7,8,9])
     *   , chunk2 = new Uint8Array([10,11,12,13,14,15,16,17,18,19]);
     *
     * const deflate = new pako.Deflate({ level: 3});
     *
     * deflate.push(chunk1, false);
     * deflate.push(chunk2, true);  // true -> last chunk
     *
     * if (deflate.err) { throw new Error(deflate.err); }
     *
     * console.log(deflate.result);
     * ```
     **/
    function Deflate$1(options) {
      this.options = common.assign({
        level: Z_DEFAULT_COMPRESSION,
        method: Z_DEFLATED$1,
        chunkSize: 16384,
        windowBits: 15,
        memLevel: 8,
        strategy: Z_DEFAULT_STRATEGY
      }, options || {});

      let opt = this.options;

      if (opt.raw && (opt.windowBits > 0)) {
        opt.windowBits = -opt.windowBits;
      }

      else if (opt.gzip && (opt.windowBits > 0) && (opt.windowBits < 16)) {
        opt.windowBits += 16;
      }

      this.err    = 0;      // error code, if happens (0 = Z_OK)
      this.msg    = '';     // error message
      this.ended  = false;  // used to avoid multiple onEnd() calls
      this.chunks = [];     // chunks of compressed data

      this.strm = new zstream();
      this.strm.avail_out = 0;

      let status = deflate_1$2.deflateInit2(
        this.strm,
        opt.level,
        opt.method,
        opt.windowBits,
        opt.memLevel,
        opt.strategy
      );

      if (status !== Z_OK$2) {
        throw new Error(messages[status]);
      }

      if (opt.header) {
        deflate_1$2.deflateSetHeader(this.strm, opt.header);
      }

      if (opt.dictionary) {
        let dict;
        // Convert data if needed
        if (typeof opt.dictionary === 'string') {
          // If we need to compress text, change encoding to utf8.
          dict = strings.string2buf(opt.dictionary);
        } else if (toString$1.call(opt.dictionary) === '[object ArrayBuffer]') {
          dict = new Uint8Array(opt.dictionary);
        } else {
          dict = opt.dictionary;
        }

        status = deflate_1$2.deflateSetDictionary(this.strm, dict);

        if (status !== Z_OK$2) {
          throw new Error(messages[status]);
        }

        this._dict_set = true;
      }
    }

    /**
     * Deflate#push(data[, flush_mode]) -> Boolean
     * - data (Uint8Array|ArrayBuffer|String): input data. Strings will be
     *   converted to utf8 byte sequence.
     * - flush_mode (Number|Boolean): 0..6 for corresponding Z_NO_FLUSH..Z_TREE modes.
     *   See constants. Skipped or `false` means Z_NO_FLUSH, `true` means Z_FINISH.
     *
     * Sends input data to deflate pipe, generating [[Deflate#onData]] calls with
     * new compressed chunks. Returns `true` on success. The last data block must
     * have `flush_mode` Z_FINISH (or `true`). That will flush internal pending
     * buffers and call [[Deflate#onEnd]].
     *
     * On fail call [[Deflate#onEnd]] with error code and return false.
     *
     * ##### Example
     *
     * ```javascript
     * push(chunk, false); // push one of data chunks
     * ...
     * push(chunk, true);  // push last chunk
     * ```
     **/
    Deflate$1.prototype.push = function (data, flush_mode) {
      const strm = this.strm;
      const chunkSize = this.options.chunkSize;
      let status, _flush_mode;

      if (this.ended) { return false; }

      if (flush_mode === ~~flush_mode) _flush_mode = flush_mode;
      else _flush_mode = flush_mode === true ? Z_FINISH$2 : Z_NO_FLUSH$1;

      // Convert data if needed
      if (typeof data === 'string') {
        // If we need to compress text, change encoding to utf8.
        strm.input = strings.string2buf(data);
      } else if (toString$1.call(data) === '[object ArrayBuffer]') {
        strm.input = new Uint8Array(data);
      } else {
        strm.input = data;
      }

      strm.next_in = 0;
      strm.avail_in = strm.input.length;

      for (;;) {
        if (strm.avail_out === 0) {
          strm.output = new Uint8Array(chunkSize);
          strm.next_out = 0;
          strm.avail_out = chunkSize;
        }

        // Make sure avail_out > 6 to avoid repeating markers
        if ((_flush_mode === Z_SYNC_FLUSH || _flush_mode === Z_FULL_FLUSH) && strm.avail_out <= 6) {
          this.onData(strm.output.subarray(0, strm.next_out));
          strm.avail_out = 0;
          continue;
        }

        status = deflate_1$2.deflate(strm, _flush_mode);

        // Ended => flush and finish
        if (status === Z_STREAM_END$2) {
          if (strm.next_out > 0) {
            this.onData(strm.output.subarray(0, strm.next_out));
          }
          status = deflate_1$2.deflateEnd(this.strm);
          this.onEnd(status);
          this.ended = true;
          return status === Z_OK$2;
        }

        // Flush if out buffer full
        if (strm.avail_out === 0) {
          this.onData(strm.output);
          continue;
        }

        // Flush if requested and has data
        if (_flush_mode > 0 && strm.next_out > 0) {
          this.onData(strm.output.subarray(0, strm.next_out));
          strm.avail_out = 0;
          continue;
        }

        if (strm.avail_in === 0) break;
      }

      return true;
    };


    /**
     * Deflate#onData(chunk) -> Void
     * - chunk (Uint8Array): output data.
     *
     * By default, stores data blocks in `chunks[]` property and glue
     * those in `onEnd`. Override this handler, if you need another behaviour.
     **/
    Deflate$1.prototype.onData = function (chunk) {
      this.chunks.push(chunk);
    };


    /**
     * Deflate#onEnd(status) -> Void
     * - status (Number): deflate status. 0 (Z_OK) on success,
     *   other if not.
     *
     * Called once after you tell deflate that the input stream is
     * complete (Z_FINISH). By default - join collected chunks,
     * free memory and fill `results` / `err` properties.
     **/
    Deflate$1.prototype.onEnd = function (status) {
      // On success - join
      if (status === Z_OK$2) {
        this.result = common.flattenChunks(this.chunks);
      }
      this.chunks = [];
      this.err = status;
      this.msg = this.strm.msg;
    };


    /**
     * deflate(data[, options]) -> Uint8Array
     * - data (Uint8Array|ArrayBuffer|String): input data to compress.
     * - options (Object): zlib deflate options.
     *
     * Compress `data` with deflate algorithm and `options`.
     *
     * Supported options are:
     *
     * - level
     * - windowBits
     * - memLevel
     * - strategy
     * - dictionary
     *
     * [http://zlib.net/manual.html#Advanced](http://zlib.net/manual.html#Advanced)
     * for more information on these.
     *
     * Sugar (options):
     *
     * - `raw` (Boolean) - say that we work with raw stream, if you don't wish to specify
     *   negative windowBits implicitly.
     *
     * ##### Example:
     *
     * ```javascript
     * const pako = require('pako')
     * const data = new Uint8Array([1,2,3,4,5,6,7,8,9]);
     *
     * console.log(pako.deflate(data));
     * ```
     **/
    function deflate$1(input, options) {
      const deflator = new Deflate$1(options);

      deflator.push(input, true);

      // That will never happens, if you don't cheat with options :)
      if (deflator.err) { throw deflator.msg || messages[deflator.err]; }

      return deflator.result;
    }


    /**
     * deflateRaw(data[, options]) -> Uint8Array
     * - data (Uint8Array|ArrayBuffer|String): input data to compress.
     * - options (Object): zlib deflate options.
     *
     * The same as [[deflate]], but creates raw data, without wrapper
     * (header and adler32 crc).
     **/
    function deflateRaw$1(input, options) {
      options = options || {};
      options.raw = true;
      return deflate$1(input, options);
    }


    /**
     * gzip(data[, options]) -> Uint8Array
     * - data (Uint8Array|ArrayBuffer|String): input data to compress.
     * - options (Object): zlib deflate options.
     *
     * The same as [[deflate]], but create gzip wrapper instead of
     * deflate one.
     **/
    function gzip$1(input, options) {
      options = options || {};
      options.gzip = true;
      return deflate$1(input, options);
    }


    var Deflate_1$1 = Deflate$1;
    var deflate_2 = deflate$1;
    var deflateRaw_1$1 = deflateRaw$1;
    var gzip_1$1 = gzip$1;
    var constants$1 = constants$2;

    var deflate_1$1 = {
    	Deflate: Deflate_1$1,
    	deflate: deflate_2,
    	deflateRaw: deflateRaw_1$1,
    	gzip: gzip_1$1,
    	constants: constants$1
    };

    // (C) 1995-2013 Jean-loup Gailly and Mark Adler
    // (C) 2014-2017 Vitaly Puzrin and Andrey Tupitsin
    //
    // This software is provided 'as-is', without any express or implied
    // warranty. In no event will the authors be held liable for any damages
    // arising from the use of this software.
    //
    // Permission is granted to anyone to use this software for any purpose,
    // including commercial applications, and to alter it and redistribute it
    // freely, subject to the following restrictions:
    //
    // 1. The origin of this software must not be misrepresented; you must not
    //   claim that you wrote the original software. If you use this software
    //   in a product, an acknowledgment in the product documentation would be
    //   appreciated but is not required.
    // 2. Altered source versions must be plainly marked as such, and must not be
    //   misrepresented as being the original software.
    // 3. This notice may not be removed or altered from any source distribution.

    // See state defs from inflate.js
    const BAD$1 = 16209;       /* got a data error -- remain here until reset */
    const TYPE$1 = 16191;      /* i: waiting for type bits, including last-flag bit */

    /*
       Decode literal, length, and distance codes and write out the resulting
       literal and match bytes until either not enough input or output is
       available, an end-of-block is encountered, or a data error is encountered.
       When large enough input and output buffers are supplied to inflate(), for
       example, a 16K input buffer and a 64K output buffer, more than 95% of the
       inflate execution time is spent in this routine.

       Entry assumptions:

            state.mode === LEN
            strm.avail_in >= 6
            strm.avail_out >= 258
            start >= strm.avail_out
            state.bits < 8

       On return, state.mode is one of:

            LEN -- ran out of enough output space or enough available input
            TYPE -- reached end of block code, inflate() to interpret next block
            BAD -- error in block data

       Notes:

        - The maximum input bits used by a length/distance pair is 15 bits for the
          length code, 5 bits for the length extra, 15 bits for the distance code,
          and 13 bits for the distance extra.  This totals 48 bits, or six bytes.
          Therefore if strm.avail_in >= 6, then there is enough input to avoid
          checking for available input while decoding.

        - The maximum bytes that a single length/distance pair can output is 258
          bytes, which is the maximum length that can be coded.  inflate_fast()
          requires strm.avail_out >= 258 for each loop to avoid checking for
          output space.
     */
    var inffast = function inflate_fast(strm, start) {
      let _in;                    /* local strm.input */
      let last;                   /* have enough input while in < last */
      let _out;                   /* local strm.output */
      let beg;                    /* inflate()'s initial strm.output */
      let end;                    /* while out < end, enough space available */
    //#ifdef INFLATE_STRICT
      let dmax;                   /* maximum distance from zlib header */
    //#endif
      let wsize;                  /* window size or zero if not using window */
      let whave;                  /* valid bytes in the window */
      let wnext;                  /* window write index */
      // Use `s_window` instead `window`, avoid conflict with instrumentation tools
      let s_window;               /* allocated sliding window, if wsize != 0 */
      let hold;                   /* local strm.hold */
      let bits;                   /* local strm.bits */
      let lcode;                  /* local strm.lencode */
      let dcode;                  /* local strm.distcode */
      let lmask;                  /* mask for first level of length codes */
      let dmask;                  /* mask for first level of distance codes */
      let here;                   /* retrieved table entry */
      let op;                     /* code bits, operation, extra bits, or */
                                  /*  window position, window bytes to copy */
      let len;                    /* match length, unused bytes */
      let dist;                   /* match distance */
      let from;                   /* where to copy match from */
      let from_source;


      let input, output; // JS specific, because we have no pointers

      /* copy state to local variables */
      const state = strm.state;
      //here = state.here;
      _in = strm.next_in;
      input = strm.input;
      last = _in + (strm.avail_in - 5);
      _out = strm.next_out;
      output = strm.output;
      beg = _out - (start - strm.avail_out);
      end = _out + (strm.avail_out - 257);
    //#ifdef INFLATE_STRICT
      dmax = state.dmax;
    //#endif
      wsize = state.wsize;
      whave = state.whave;
      wnext = state.wnext;
      s_window = state.window;
      hold = state.hold;
      bits = state.bits;
      lcode = state.lencode;
      dcode = state.distcode;
      lmask = (1 << state.lenbits) - 1;
      dmask = (1 << state.distbits) - 1;


      /* decode literals and length/distances until end-of-block or not enough
         input data or output space */

      top:
      do {
        if (bits < 15) {
          hold += input[_in++] << bits;
          bits += 8;
          hold += input[_in++] << bits;
          bits += 8;
        }

        here = lcode[hold & lmask];

        dolen:
        for (;;) { // Goto emulation
          op = here >>> 24/*here.bits*/;
          hold >>>= op;
          bits -= op;
          op = (here >>> 16) & 0xff/*here.op*/;
          if (op === 0) {                          /* literal */
            //Tracevv((stderr, here.val >= 0x20 && here.val < 0x7f ?
            //        "inflate:         literal '%c'\n" :
            //        "inflate:         literal 0x%02x\n", here.val));
            output[_out++] = here & 0xffff/*here.val*/;
          }
          else if (op & 16) {                     /* length base */
            len = here & 0xffff/*here.val*/;
            op &= 15;                           /* number of extra bits */
            if (op) {
              if (bits < op) {
                hold += input[_in++] << bits;
                bits += 8;
              }
              len += hold & ((1 << op) - 1);
              hold >>>= op;
              bits -= op;
            }
            //Tracevv((stderr, "inflate:         length %u\n", len));
            if (bits < 15) {
              hold += input[_in++] << bits;
              bits += 8;
              hold += input[_in++] << bits;
              bits += 8;
            }
            here = dcode[hold & dmask];

            dodist:
            for (;;) { // goto emulation
              op = here >>> 24/*here.bits*/;
              hold >>>= op;
              bits -= op;
              op = (here >>> 16) & 0xff/*here.op*/;

              if (op & 16) {                      /* distance base */
                dist = here & 0xffff/*here.val*/;
                op &= 15;                       /* number of extra bits */
                if (bits < op) {
                  hold += input[_in++] << bits;
                  bits += 8;
                  if (bits < op) {
                    hold += input[_in++] << bits;
                    bits += 8;
                  }
                }
                dist += hold & ((1 << op) - 1);
    //#ifdef INFLATE_STRICT
                if (dist > dmax) {
                  strm.msg = 'invalid distance too far back';
                  state.mode = BAD$1;
                  break top;
                }
    //#endif
                hold >>>= op;
                bits -= op;
                //Tracevv((stderr, "inflate:         distance %u\n", dist));
                op = _out - beg;                /* max distance in output */
                if (dist > op) {                /* see if copy from window */
                  op = dist - op;               /* distance back in window */
                  if (op > whave) {
                    if (state.sane) {
                      strm.msg = 'invalid distance too far back';
                      state.mode = BAD$1;
                      break top;
                    }

    // (!) This block is disabled in zlib defaults,
    // don't enable it for binary compatibility
    //#ifdef INFLATE_ALLOW_INVALID_DISTANCE_TOOFAR_ARRR
    //                if (len <= op - whave) {
    //                  do {
    //                    output[_out++] = 0;
    //                  } while (--len);
    //                  continue top;
    //                }
    //                len -= op - whave;
    //                do {
    //                  output[_out++] = 0;
    //                } while (--op > whave);
    //                if (op === 0) {
    //                  from = _out - dist;
    //                  do {
    //                    output[_out++] = output[from++];
    //                  } while (--len);
    //                  continue top;
    //                }
    //#endif
                  }
                  from = 0; // window index
                  from_source = s_window;
                  if (wnext === 0) {           /* very common case */
                    from += wsize - op;
                    if (op < len) {         /* some from window */
                      len -= op;
                      do {
                        output[_out++] = s_window[from++];
                      } while (--op);
                      from = _out - dist;  /* rest from output */
                      from_source = output;
                    }
                  }
                  else if (wnext < op) {      /* wrap around window */
                    from += wsize + wnext - op;
                    op -= wnext;
                    if (op < len) {         /* some from end of window */
                      len -= op;
                      do {
                        output[_out++] = s_window[from++];
                      } while (--op);
                      from = 0;
                      if (wnext < len) {  /* some from start of window */
                        op = wnext;
                        len -= op;
                        do {
                          output[_out++] = s_window[from++];
                        } while (--op);
                        from = _out - dist;      /* rest from output */
                        from_source = output;
                      }
                    }
                  }
                  else {                      /* contiguous in window */
                    from += wnext - op;
                    if (op < len) {         /* some from window */
                      len -= op;
                      do {
                        output[_out++] = s_window[from++];
                      } while (--op);
                      from = _out - dist;  /* rest from output */
                      from_source = output;
                    }
                  }
                  while (len > 2) {
                    output[_out++] = from_source[from++];
                    output[_out++] = from_source[from++];
                    output[_out++] = from_source[from++];
                    len -= 3;
                  }
                  if (len) {
                    output[_out++] = from_source[from++];
                    if (len > 1) {
                      output[_out++] = from_source[from++];
                    }
                  }
                }
                else {
                  from = _out - dist;          /* copy direct from output */
                  do {                        /* minimum length is three */
                    output[_out++] = output[from++];
                    output[_out++] = output[from++];
                    output[_out++] = output[from++];
                    len -= 3;
                  } while (len > 2);
                  if (len) {
                    output[_out++] = output[from++];
                    if (len > 1) {
                      output[_out++] = output[from++];
                    }
                  }
                }
              }
              else if ((op & 64) === 0) {          /* 2nd level distance code */
                here = dcode[(here & 0xffff)/*here.val*/ + (hold & ((1 << op) - 1))];
                continue dodist;
              }
              else {
                strm.msg = 'invalid distance code';
                state.mode = BAD$1;
                break top;
              }

              break; // need to emulate goto via "continue"
            }
          }
          else if ((op & 64) === 0) {              /* 2nd level length code */
            here = lcode[(here & 0xffff)/*here.val*/ + (hold & ((1 << op) - 1))];
            continue dolen;
          }
          else if (op & 32) {                     /* end-of-block */
            //Tracevv((stderr, "inflate:         end of block\n"));
            state.mode = TYPE$1;
            break top;
          }
          else {
            strm.msg = 'invalid literal/length code';
            state.mode = BAD$1;
            break top;
          }

          break; // need to emulate goto via "continue"
        }
      } while (_in < last && _out < end);

      /* return unused bytes (on entry, bits < 8, so in won't go too far back) */
      len = bits >> 3;
      _in -= len;
      bits -= len << 3;
      hold &= (1 << bits) - 1;

      /* update state and return */
      strm.next_in = _in;
      strm.next_out = _out;
      strm.avail_in = (_in < last ? 5 + (last - _in) : 5 - (_in - last));
      strm.avail_out = (_out < end ? 257 + (end - _out) : 257 - (_out - end));
      state.hold = hold;
      state.bits = bits;
      return;
    };

    // (C) 1995-2013 Jean-loup Gailly and Mark Adler
    // (C) 2014-2017 Vitaly Puzrin and Andrey Tupitsin
    //
    // This software is provided 'as-is', without any express or implied
    // warranty. In no event will the authors be held liable for any damages
    // arising from the use of this software.
    //
    // Permission is granted to anyone to use this software for any purpose,
    // including commercial applications, and to alter it and redistribute it
    // freely, subject to the following restrictions:
    //
    // 1. The origin of this software must not be misrepresented; you must not
    //   claim that you wrote the original software. If you use this software
    //   in a product, an acknowledgment in the product documentation would be
    //   appreciated but is not required.
    // 2. Altered source versions must be plainly marked as such, and must not be
    //   misrepresented as being the original software.
    // 3. This notice may not be removed or altered from any source distribution.

    const MAXBITS = 15;
    const ENOUGH_LENS$1 = 852;
    const ENOUGH_DISTS$1 = 592;
    //const ENOUGH = (ENOUGH_LENS+ENOUGH_DISTS);

    const CODES$1 = 0;
    const LENS$1 = 1;
    const DISTS$1 = 2;

    const lbase = new Uint16Array([ /* Length codes 257..285 base */
      3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31,
      35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258, 0, 0
    ]);

    const lext = new Uint8Array([ /* Length codes 257..285 extra */
      16, 16, 16, 16, 16, 16, 16, 16, 17, 17, 17, 17, 18, 18, 18, 18,
      19, 19, 19, 19, 20, 20, 20, 20, 21, 21, 21, 21, 16, 72, 78
    ]);

    const dbase = new Uint16Array([ /* Distance codes 0..29 base */
      1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193,
      257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145,
      8193, 12289, 16385, 24577, 0, 0
    ]);

    const dext = new Uint8Array([ /* Distance codes 0..29 extra */
      16, 16, 16, 16, 17, 17, 18, 18, 19, 19, 20, 20, 21, 21, 22, 22,
      23, 23, 24, 24, 25, 25, 26, 26, 27, 27,
      28, 28, 29, 29, 64, 64
    ]);

    const inflate_table = (type, lens, lens_index, codes, table, table_index, work, opts) =>
    {
      const bits = opts.bits;
          //here = opts.here; /* table entry for duplication */

      let len = 0;               /* a code's length in bits */
      let sym = 0;               /* index of code symbols */
      let min = 0, max = 0;          /* minimum and maximum code lengths */
      let root = 0;              /* number of index bits for root table */
      let curr = 0;              /* number of index bits for current table */
      let drop = 0;              /* code bits to drop for sub-table */
      let left = 0;                   /* number of prefix codes available */
      let used = 0;              /* code entries in table used */
      let huff = 0;              /* Huffman code */
      let incr;              /* for incrementing code, index */
      let fill;              /* index for replicating entries */
      let low;               /* low bits for current root entry */
      let mask;              /* mask for low root bits */
      let next;             /* next available space in table */
      let base = null;     /* base value table to use */
    //  let shoextra;    /* extra bits table to use */
      let match;                  /* use base and extra for symbol >= match */
      const count = new Uint16Array(MAXBITS + 1); //[MAXBITS+1];    /* number of codes of each length */
      const offs = new Uint16Array(MAXBITS + 1); //[MAXBITS+1];     /* offsets in table for each length */
      let extra = null;

      let here_bits, here_op, here_val;

      /*
       Process a set of code lengths to create a canonical Huffman code.  The
       code lengths are lens[0..codes-1].  Each length corresponds to the
       symbols 0..codes-1.  The Huffman code is generated by first sorting the
       symbols by length from short to long, and retaining the symbol order
       for codes with equal lengths.  Then the code starts with all zero bits
       for the first code of the shortest length, and the codes are integer
       increments for the same length, and zeros are appended as the length
       increases.  For the deflate format, these bits are stored backwards
       from their more natural integer increment ordering, and so when the
       decoding tables are built in the large loop below, the integer codes
       are incremented backwards.

       This routine assumes, but does not check, that all of the entries in
       lens[] are in the range 0..MAXBITS.  The caller must assure this.
       1..MAXBITS is interpreted as that code length.  zero means that that
       symbol does not occur in this code.

       The codes are sorted by computing a count of codes for each length,
       creating from that a table of starting indices for each length in the
       sorted table, and then entering the symbols in order in the sorted
       table.  The sorted table is work[], with that space being provided by
       the caller.

       The length counts are used for other purposes as well, i.e. finding
       the minimum and maximum length codes, determining if there are any
       codes at all, checking for a valid set of lengths, and looking ahead
       at length counts to determine sub-table sizes when building the
       decoding tables.
       */

      /* accumulate lengths for codes (assumes lens[] all in 0..MAXBITS) */
      for (len = 0; len <= MAXBITS; len++) {
        count[len] = 0;
      }
      for (sym = 0; sym < codes; sym++) {
        count[lens[lens_index + sym]]++;
      }

      /* bound code lengths, force root to be within code lengths */
      root = bits;
      for (max = MAXBITS; max >= 1; max--) {
        if (count[max] !== 0) { break; }
      }
      if (root > max) {
        root = max;
      }
      if (max === 0) {                     /* no symbols to code at all */
        //table.op[opts.table_index] = 64;  //here.op = (var char)64;    /* invalid code marker */
        //table.bits[opts.table_index] = 1;   //here.bits = (var char)1;
        //table.val[opts.table_index++] = 0;   //here.val = (var short)0;
        table[table_index++] = (1 << 24) | (64 << 16) | 0;


        //table.op[opts.table_index] = 64;
        //table.bits[opts.table_index] = 1;
        //table.val[opts.table_index++] = 0;
        table[table_index++] = (1 << 24) | (64 << 16) | 0;

        opts.bits = 1;
        return 0;     /* no symbols, but wait for decoding to report error */
      }
      for (min = 1; min < max; min++) {
        if (count[min] !== 0) { break; }
      }
      if (root < min) {
        root = min;
      }

      /* check for an over-subscribed or incomplete set of lengths */
      left = 1;
      for (len = 1; len <= MAXBITS; len++) {
        left <<= 1;
        left -= count[len];
        if (left < 0) {
          return -1;
        }        /* over-subscribed */
      }
      if (left > 0 && (type === CODES$1 || max !== 1)) {
        return -1;                      /* incomplete set */
      }

      /* generate offsets into symbol table for each length for sorting */
      offs[1] = 0;
      for (len = 1; len < MAXBITS; len++) {
        offs[len + 1] = offs[len] + count[len];
      }

      /* sort symbols by length, by symbol order within each length */
      for (sym = 0; sym < codes; sym++) {
        if (lens[lens_index + sym] !== 0) {
          work[offs[lens[lens_index + sym]]++] = sym;
        }
      }

      /*
       Create and fill in decoding tables.  In this loop, the table being
       filled is at next and has curr index bits.  The code being used is huff
       with length len.  That code is converted to an index by dropping drop
       bits off of the bottom.  For codes where len is less than drop + curr,
       those top drop + curr - len bits are incremented through all values to
       fill the table with replicated entries.

       root is the number of index bits for the root table.  When len exceeds
       root, sub-tables are created pointed to by the root entry with an index
       of the low root bits of huff.  This is saved in low to check for when a
       new sub-table should be started.  drop is zero when the root table is
       being filled, and drop is root when sub-tables are being filled.

       When a new sub-table is needed, it is necessary to look ahead in the
       code lengths to determine what size sub-table is needed.  The length
       counts are used for this, and so count[] is decremented as codes are
       entered in the tables.

       used keeps track of how many table entries have been allocated from the
       provided *table space.  It is checked for LENS and DIST tables against
       the constants ENOUGH_LENS and ENOUGH_DISTS to guard against changes in
       the initial root table size constants.  See the comments in inftrees.h
       for more information.

       sym increments through all symbols, and the loop terminates when
       all codes of length max, i.e. all codes, have been processed.  This
       routine permits incomplete codes, so another loop after this one fills
       in the rest of the decoding tables with invalid code markers.
       */

      /* set up for code type */
      // poor man optimization - use if-else instead of switch,
      // to avoid deopts in old v8
      if (type === CODES$1) {
        base = extra = work;    /* dummy value--not used */
        match = 20;

      } else if (type === LENS$1) {
        base = lbase;
        extra = lext;
        match = 257;

      } else {                    /* DISTS */
        base = dbase;
        extra = dext;
        match = 0;
      }

      /* initialize opts for loop */
      huff = 0;                   /* starting code */
      sym = 0;                    /* starting code symbol */
      len = min;                  /* starting code length */
      next = table_index;              /* current table to fill in */
      curr = root;                /* current table index bits */
      drop = 0;                   /* current bits to drop from code for index */
      low = -1;                   /* trigger new sub-table when len > root */
      used = 1 << root;          /* use root table entries */
      mask = used - 1;            /* mask for comparing low */

      /* check available table space */
      if ((type === LENS$1 && used > ENOUGH_LENS$1) ||
        (type === DISTS$1 && used > ENOUGH_DISTS$1)) {
        return 1;
      }

      /* process all codes and make table entries */
      for (;;) {
        /* create table entry */
        here_bits = len - drop;
        if (work[sym] + 1 < match) {
          here_op = 0;
          here_val = work[sym];
        }
        else if (work[sym] >= match) {
          here_op = extra[work[sym] - match];
          here_val = base[work[sym] - match];
        }
        else {
          here_op = 32 + 64;         /* end of block */
          here_val = 0;
        }

        /* replicate for those indices with low len bits equal to huff */
        incr = 1 << (len - drop);
        fill = 1 << curr;
        min = fill;                 /* save offset to next table */
        do {
          fill -= incr;
          table[next + (huff >> drop) + fill] = (here_bits << 24) | (here_op << 16) | here_val |0;
        } while (fill !== 0);

        /* backwards increment the len-bit code huff */
        incr = 1 << (len - 1);
        while (huff & incr) {
          incr >>= 1;
        }
        if (incr !== 0) {
          huff &= incr - 1;
          huff += incr;
        } else {
          huff = 0;
        }

        /* go to next symbol, update count, len */
        sym++;
        if (--count[len] === 0) {
          if (len === max) { break; }
          len = lens[lens_index + work[sym]];
        }

        /* create new sub-table if needed */
        if (len > root && (huff & mask) !== low) {
          /* if first time, transition to sub-tables */
          if (drop === 0) {
            drop = root;
          }

          /* increment past last table */
          next += min;            /* here min is 1 << curr */

          /* determine length of next table */
          curr = len - drop;
          left = 1 << curr;
          while (curr + drop < max) {
            left -= count[curr + drop];
            if (left <= 0) { break; }
            curr++;
            left <<= 1;
          }

          /* check for enough space */
          used += 1 << curr;
          if ((type === LENS$1 && used > ENOUGH_LENS$1) ||
            (type === DISTS$1 && used > ENOUGH_DISTS$1)) {
            return 1;
          }

          /* point entry in root table to sub-table */
          low = huff & mask;
          /*table.op[low] = curr;
          table.bits[low] = root;
          table.val[low] = next - opts.table_index;*/
          table[low] = (root << 24) | (curr << 16) | (next - table_index) |0;
        }
      }

      /* fill in remaining table entry if code is incomplete (guaranteed to have
       at most one remaining entry, since if the code is incomplete, the
       maximum code length that was allowed to get this far is one bit) */
      if (huff !== 0) {
        //table.op[next + huff] = 64;            /* invalid code marker */
        //table.bits[next + huff] = len - drop;
        //table.val[next + huff] = 0;
        table[next + huff] = ((len - drop) << 24) | (64 << 16) |0;
      }

      /* set return parameters */
      //opts.table_index += used;
      opts.bits = root;
      return 0;
    };


    var inftrees = inflate_table;

    // (C) 1995-2013 Jean-loup Gailly and Mark Adler
    // (C) 2014-2017 Vitaly Puzrin and Andrey Tupitsin
    //
    // This software is provided 'as-is', without any express or implied
    // warranty. In no event will the authors be held liable for any damages
    // arising from the use of this software.
    //
    // Permission is granted to anyone to use this software for any purpose,
    // including commercial applications, and to alter it and redistribute it
    // freely, subject to the following restrictions:
    //
    // 1. The origin of this software must not be misrepresented; you must not
    //   claim that you wrote the original software. If you use this software
    //   in a product, an acknowledgment in the product documentation would be
    //   appreciated but is not required.
    // 2. Altered source versions must be plainly marked as such, and must not be
    //   misrepresented as being the original software.
    // 3. This notice may not be removed or altered from any source distribution.






    const CODES = 0;
    const LENS = 1;
    const DISTS = 2;

    /* Public constants ==========================================================*/
    /* ===========================================================================*/

    const {
      Z_FINISH: Z_FINISH$1, Z_BLOCK, Z_TREES,
      Z_OK: Z_OK$1, Z_STREAM_END: Z_STREAM_END$1, Z_NEED_DICT: Z_NEED_DICT$1, Z_STREAM_ERROR: Z_STREAM_ERROR$1, Z_DATA_ERROR: Z_DATA_ERROR$1, Z_MEM_ERROR: Z_MEM_ERROR$1, Z_BUF_ERROR,
      Z_DEFLATED
    } = constants$2;


    /* STATES ====================================================================*/
    /* ===========================================================================*/


    const    HEAD = 16180;       /* i: waiting for magic header */
    const    FLAGS = 16181;      /* i: waiting for method and flags (gzip) */
    const    TIME = 16182;       /* i: waiting for modification time (gzip) */
    const    OS = 16183;         /* i: waiting for extra flags and operating system (gzip) */
    const    EXLEN = 16184;      /* i: waiting for extra length (gzip) */
    const    EXTRA = 16185;      /* i: waiting for extra bytes (gzip) */
    const    NAME = 16186;       /* i: waiting for end of file name (gzip) */
    const    COMMENT = 16187;    /* i: waiting for end of comment (gzip) */
    const    HCRC = 16188;       /* i: waiting for header crc (gzip) */
    const    DICTID = 16189;    /* i: waiting for dictionary check value */
    const    DICT = 16190;      /* waiting for inflateSetDictionary() call */
    const        TYPE = 16191;      /* i: waiting for type bits, including last-flag bit */
    const        TYPEDO = 16192;    /* i: same, but skip check to exit inflate on new block */
    const        STORED = 16193;    /* i: waiting for stored size (length and complement) */
    const        COPY_ = 16194;     /* i/o: same as COPY below, but only first time in */
    const        COPY = 16195;      /* i/o: waiting for input or output to copy stored block */
    const        TABLE = 16196;     /* i: waiting for dynamic block table lengths */
    const        LENLENS = 16197;   /* i: waiting for code length code lengths */
    const        CODELENS = 16198;  /* i: waiting for length/lit and distance code lengths */
    const            LEN_ = 16199;      /* i: same as LEN below, but only first time in */
    const            LEN = 16200;       /* i: waiting for length/lit/eob code */
    const            LENEXT = 16201;    /* i: waiting for length extra bits */
    const            DIST = 16202;      /* i: waiting for distance code */
    const            DISTEXT = 16203;   /* i: waiting for distance extra bits */
    const            MATCH = 16204;     /* o: waiting for output space to copy string */
    const            LIT = 16205;       /* o: waiting for output space to write literal */
    const    CHECK = 16206;     /* i: waiting for 32-bit check value */
    const    LENGTH = 16207;    /* i: waiting for 32-bit length (gzip) */
    const    DONE = 16208;      /* finished check, done -- remain here until reset */
    const    BAD = 16209;       /* got a data error -- remain here until reset */
    const    MEM = 16210;       /* got an inflate() memory error -- remain here until reset */
    const    SYNC = 16211;      /* looking for synchronization bytes to restart inflate() */

    /* ===========================================================================*/



    const ENOUGH_LENS = 852;
    const ENOUGH_DISTS = 592;
    //const ENOUGH =  (ENOUGH_LENS+ENOUGH_DISTS);

    const MAX_WBITS = 15;
    /* 32K LZ77 window */
    const DEF_WBITS = MAX_WBITS;


    const zswap32 = (q) => {

      return  (((q >>> 24) & 0xff) +
              ((q >>> 8) & 0xff00) +
              ((q & 0xff00) << 8) +
              ((q & 0xff) << 24));
    };


    function InflateState() {
      this.strm = null;           /* pointer back to this zlib stream */
      this.mode = 0;              /* current inflate mode */
      this.last = false;          /* true if processing last block */
      this.wrap = 0;              /* bit 0 true for zlib, bit 1 true for gzip,
                                     bit 2 true to validate check value */
      this.havedict = false;      /* true if dictionary provided */
      this.flags = 0;             /* gzip header method and flags (0 if zlib), or
                                     -1 if raw or no header yet */
      this.dmax = 0;              /* zlib header max distance (INFLATE_STRICT) */
      this.check = 0;             /* protected copy of check value */
      this.total = 0;             /* protected copy of output count */
      // TODO: may be {}
      this.head = null;           /* where to save gzip header information */

      /* sliding window */
      this.wbits = 0;             /* log base 2 of requested window size */
      this.wsize = 0;             /* window size or zero if not using window */
      this.whave = 0;             /* valid bytes in the window */
      this.wnext = 0;             /* window write index */
      this.window = null;         /* allocated sliding window, if needed */

      /* bit accumulator */
      this.hold = 0;              /* input bit accumulator */
      this.bits = 0;              /* number of bits in "in" */

      /* for string and stored block copying */
      this.length = 0;            /* literal or length of data to copy */
      this.offset = 0;            /* distance back to copy string from */

      /* for table and code decoding */
      this.extra = 0;             /* extra bits needed */

      /* fixed and dynamic code tables */
      this.lencode = null;          /* starting table for length/literal codes */
      this.distcode = null;         /* starting table for distance codes */
      this.lenbits = 0;           /* index bits for lencode */
      this.distbits = 0;          /* index bits for distcode */

      /* dynamic table building */
      this.ncode = 0;             /* number of code length code lengths */
      this.nlen = 0;              /* number of length code lengths */
      this.ndist = 0;             /* number of distance code lengths */
      this.have = 0;              /* number of code lengths in lens[] */
      this.next = null;              /* next available space in codes[] */

      this.lens = new Uint16Array(320); /* temporary storage for code lengths */
      this.work = new Uint16Array(288); /* work area for code table building */

      /*
       because we don't have pointers in js, we use lencode and distcode directly
       as buffers so we don't need codes
      */
      //this.codes = new Int32Array(ENOUGH);       /* space for code tables */
      this.lendyn = null;              /* dynamic table for length/literal codes (JS specific) */
      this.distdyn = null;             /* dynamic table for distance codes (JS specific) */
      this.sane = 0;                   /* if false, allow invalid distance too far */
      this.back = 0;                   /* bits back of last unprocessed length/lit */
      this.was = 0;                    /* initial length of match */
    }


    const inflateStateCheck = (strm) => {

      if (!strm) {
        return 1;
      }
      const state = strm.state;
      if (!state || state.strm !== strm ||
        state.mode < HEAD || state.mode > SYNC) {
        return 1;
      }
      return 0;
    };


    const inflateResetKeep = (strm) => {

      if (inflateStateCheck(strm)) { return Z_STREAM_ERROR$1; }
      const state = strm.state;
      strm.total_in = strm.total_out = state.total = 0;
      strm.msg = ''; /*Z_NULL*/
      if (state.wrap) {       /* to support ill-conceived Java test suite */
        strm.adler = state.wrap & 1;
      }
      state.mode = HEAD;
      state.last = 0;
      state.havedict = 0;
      state.flags = -1;
      state.dmax = 32768;
      state.head = null/*Z_NULL*/;
      state.hold = 0;
      state.bits = 0;
      //state.lencode = state.distcode = state.next = state.codes;
      state.lencode = state.lendyn = new Int32Array(ENOUGH_LENS);
      state.distcode = state.distdyn = new Int32Array(ENOUGH_DISTS);

      state.sane = 1;
      state.back = -1;
      //Tracev((stderr, "inflate: reset\n"));
      return Z_OK$1;
    };


    const inflateReset = (strm) => {

      if (inflateStateCheck(strm)) { return Z_STREAM_ERROR$1; }
      const state = strm.state;
      state.wsize = 0;
      state.whave = 0;
      state.wnext = 0;
      return inflateResetKeep(strm);

    };


    const inflateReset2 = (strm, windowBits) => {
      let wrap;

      /* get the state */
      if (inflateStateCheck(strm)) { return Z_STREAM_ERROR$1; }
      const state = strm.state;

      /* extract wrap request from windowBits parameter */
      if (windowBits < 0) {
        wrap = 0;
        windowBits = -windowBits;
      }
      else {
        wrap = (windowBits >> 4) + 5;
        if (windowBits < 48) {
          windowBits &= 15;
        }
      }

      /* set number of window bits, free window if different */
      if (windowBits && (windowBits < 8 || windowBits > 15)) {
        return Z_STREAM_ERROR$1;
      }
      if (state.window !== null && state.wbits !== windowBits) {
        state.window = null;
      }

      /* update state and reset the rest of it */
      state.wrap = wrap;
      state.wbits = windowBits;
      return inflateReset(strm);
    };


    const inflateInit2 = (strm, windowBits) => {

      if (!strm) { return Z_STREAM_ERROR$1; }
      //strm.msg = Z_NULL;                 /* in case we return an error */

      const state = new InflateState();

      //if (state === Z_NULL) return Z_MEM_ERROR;
      //Tracev((stderr, "inflate: allocated\n"));
      strm.state = state;
      state.strm = strm;
      state.window = null/*Z_NULL*/;
      state.mode = HEAD;     /* to pass state test in inflateReset2() */
      const ret = inflateReset2(strm, windowBits);
      if (ret !== Z_OK$1) {
        strm.state = null/*Z_NULL*/;
      }
      return ret;
    };


    const inflateInit = (strm) => {

      return inflateInit2(strm, DEF_WBITS);
    };


    /*
     Return state with length and distance decoding tables and index sizes set to
     fixed code decoding.  Normally this returns fixed tables from inffixed.h.
     If BUILDFIXED is defined, then instead this routine builds the tables the
     first time it's called, and returns those tables the first time and
     thereafter.  This reduces the size of the code by about 2K bytes, in
     exchange for a little execution time.  However, BUILDFIXED should not be
     used for threaded applications, since the rewriting of the tables and virgin
     may not be thread-safe.
     */
    let virgin = true;

    let lenfix, distfix; // We have no pointers in JS, so keep tables separate


    const fixedtables = (state) => {

      /* build fixed huffman tables if first call (may not be thread safe) */
      if (virgin) {
        lenfix = new Int32Array(512);
        distfix = new Int32Array(32);

        /* literal/length table */
        let sym = 0;
        while (sym < 144) { state.lens[sym++] = 8; }
        while (sym < 256) { state.lens[sym++] = 9; }
        while (sym < 280) { state.lens[sym++] = 7; }
        while (sym < 288) { state.lens[sym++] = 8; }

        inftrees(LENS,  state.lens, 0, 288, lenfix,   0, state.work, { bits: 9 });

        /* distance table */
        sym = 0;
        while (sym < 32) { state.lens[sym++] = 5; }

        inftrees(DISTS, state.lens, 0, 32,   distfix, 0, state.work, { bits: 5 });

        /* do this just once */
        virgin = false;
      }

      state.lencode = lenfix;
      state.lenbits = 9;
      state.distcode = distfix;
      state.distbits = 5;
    };


    /*
     Update the window with the last wsize (normally 32K) bytes written before
     returning.  If window does not exist yet, create it.  This is only called
     when a window is already in use, or when output has been written during this
     inflate call, but the end of the deflate stream has not been reached yet.
     It is also called to create a window for dictionary data when a dictionary
     is loaded.

     Providing output buffers larger than 32K to inflate() should provide a speed
     advantage, since only the last 32K of output is copied to the sliding window
     upon return from inflate(), and since all distances after the first 32K of
     output will fall in the output data, making match copies simpler and faster.
     The advantage may be dependent on the size of the processor's data caches.
     */
    const updatewindow = (strm, src, end, copy) => {

      let dist;
      const state = strm.state;

      /* if it hasn't been done already, allocate space for the window */
      if (state.window === null) {
        state.wsize = 1 << state.wbits;
        state.wnext = 0;
        state.whave = 0;

        state.window = new Uint8Array(state.wsize);
      }

      /* copy state->wsize or less output bytes into the circular window */
      if (copy >= state.wsize) {
        state.window.set(src.subarray(end - state.wsize, end), 0);
        state.wnext = 0;
        state.whave = state.wsize;
      }
      else {
        dist = state.wsize - state.wnext;
        if (dist > copy) {
          dist = copy;
        }
        //zmemcpy(state->window + state->wnext, end - copy, dist);
        state.window.set(src.subarray(end - copy, end - copy + dist), state.wnext);
        copy -= dist;
        if (copy) {
          //zmemcpy(state->window, end - copy, copy);
          state.window.set(src.subarray(end - copy, end), 0);
          state.wnext = copy;
          state.whave = state.wsize;
        }
        else {
          state.wnext += dist;
          if (state.wnext === state.wsize) { state.wnext = 0; }
          if (state.whave < state.wsize) { state.whave += dist; }
        }
      }
      return 0;
    };


    const inflate$2 = (strm, flush) => {

      let state;
      let input, output;          // input/output buffers
      let next;                   /* next input INDEX */
      let put;                    /* next output INDEX */
      let have, left;             /* available input and output */
      let hold;                   /* bit buffer */
      let bits;                   /* bits in bit buffer */
      let _in, _out;              /* save starting available input and output */
      let copy;                   /* number of stored or match bytes to copy */
      let from;                   /* where to copy match bytes from */
      let from_source;
      let here = 0;               /* current decoding table entry */
      let here_bits, here_op, here_val; // paked "here" denormalized (JS specific)
      //let last;                   /* parent table entry */
      let last_bits, last_op, last_val; // paked "last" denormalized (JS specific)
      let len;                    /* length to copy for repeats, bits to drop */
      let ret;                    /* return code */
      const hbuf = new Uint8Array(4);    /* buffer for gzip header crc calculation */
      let opts;

      let n; // temporary variable for NEED_BITS

      const order = /* permutation of code lengths */
        new Uint8Array([ 16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15 ]);


      if (inflateStateCheck(strm) || !strm.output ||
          (!strm.input && strm.avail_in !== 0)) {
        return Z_STREAM_ERROR$1;
      }

      state = strm.state;
      if (state.mode === TYPE) { state.mode = TYPEDO; }    /* skip check */


      //--- LOAD() ---
      put = strm.next_out;
      output = strm.output;
      left = strm.avail_out;
      next = strm.next_in;
      input = strm.input;
      have = strm.avail_in;
      hold = state.hold;
      bits = state.bits;
      //---

      _in = have;
      _out = left;
      ret = Z_OK$1;

      inf_leave: // goto emulation
      for (;;) {
        switch (state.mode) {
          case HEAD:
            if (state.wrap === 0) {
              state.mode = TYPEDO;
              break;
            }
            //=== NEEDBITS(16);
            while (bits < 16) {
              if (have === 0) { break inf_leave; }
              have--;
              hold += input[next++] << bits;
              bits += 8;
            }
            //===//
            if ((state.wrap & 2) && hold === 0x8b1f) {  /* gzip header */
              if (state.wbits === 0) {
                state.wbits = 15;
              }
              state.check = 0/*crc32(0L, Z_NULL, 0)*/;
              //=== CRC2(state.check, hold);
              hbuf[0] = hold & 0xff;
              hbuf[1] = (hold >>> 8) & 0xff;
              state.check = crc32_1(state.check, hbuf, 2, 0);
              //===//

              //=== INITBITS();
              hold = 0;
              bits = 0;
              //===//
              state.mode = FLAGS;
              break;
            }
            if (state.head) {
              state.head.done = false;
            }
            if (!(state.wrap & 1) ||   /* check if zlib header allowed */
              (((hold & 0xff)/*BITS(8)*/ << 8) + (hold >> 8)) % 31) {
              strm.msg = 'incorrect header check';
              state.mode = BAD;
              break;
            }
            if ((hold & 0x0f)/*BITS(4)*/ !== Z_DEFLATED) {
              strm.msg = 'unknown compression method';
              state.mode = BAD;
              break;
            }
            //--- DROPBITS(4) ---//
            hold >>>= 4;
            bits -= 4;
            //---//
            len = (hold & 0x0f)/*BITS(4)*/ + 8;
            if (state.wbits === 0) {
              state.wbits = len;
            }
            if (len > 15 || len > state.wbits) {
              strm.msg = 'invalid window size';
              state.mode = BAD;
              break;
            }

            // !!! pako patch. Force use `options.windowBits` if passed.
            // Required to always use max window size by default.
            state.dmax = 1 << state.wbits;
            //state.dmax = 1 << len;

            state.flags = 0;               /* indicate zlib header */
            //Tracev((stderr, "inflate:   zlib header ok\n"));
            strm.adler = state.check = 1/*adler32(0L, Z_NULL, 0)*/;
            state.mode = hold & 0x200 ? DICTID : TYPE;
            //=== INITBITS();
            hold = 0;
            bits = 0;
            //===//
            break;
          case FLAGS:
            //=== NEEDBITS(16); */
            while (bits < 16) {
              if (have === 0) { break inf_leave; }
              have--;
              hold += input[next++] << bits;
              bits += 8;
            }
            //===//
            state.flags = hold;
            if ((state.flags & 0xff) !== Z_DEFLATED) {
              strm.msg = 'unknown compression method';
              state.mode = BAD;
              break;
            }
            if (state.flags & 0xe000) {
              strm.msg = 'unknown header flags set';
              state.mode = BAD;
              break;
            }
            if (state.head) {
              state.head.text = ((hold >> 8) & 1);
            }
            if ((state.flags & 0x0200) && (state.wrap & 4)) {
              //=== CRC2(state.check, hold);
              hbuf[0] = hold & 0xff;
              hbuf[1] = (hold >>> 8) & 0xff;
              state.check = crc32_1(state.check, hbuf, 2, 0);
              //===//
            }
            //=== INITBITS();
            hold = 0;
            bits = 0;
            //===//
            state.mode = TIME;
            /* falls through */
          case TIME:
            //=== NEEDBITS(32); */
            while (bits < 32) {
              if (have === 0) { break inf_leave; }
              have--;
              hold += input[next++] << bits;
              bits += 8;
            }
            //===//
            if (state.head) {
              state.head.time = hold;
            }
            if ((state.flags & 0x0200) && (state.wrap & 4)) {
              //=== CRC4(state.check, hold)
              hbuf[0] = hold & 0xff;
              hbuf[1] = (hold >>> 8) & 0xff;
              hbuf[2] = (hold >>> 16) & 0xff;
              hbuf[3] = (hold >>> 24) & 0xff;
              state.check = crc32_1(state.check, hbuf, 4, 0);
              //===
            }
            //=== INITBITS();
            hold = 0;
            bits = 0;
            //===//
            state.mode = OS;
            /* falls through */
          case OS:
            //=== NEEDBITS(16); */
            while (bits < 16) {
              if (have === 0) { break inf_leave; }
              have--;
              hold += input[next++] << bits;
              bits += 8;
            }
            //===//
            if (state.head) {
              state.head.xflags = (hold & 0xff);
              state.head.os = (hold >> 8);
            }
            if ((state.flags & 0x0200) && (state.wrap & 4)) {
              //=== CRC2(state.check, hold);
              hbuf[0] = hold & 0xff;
              hbuf[1] = (hold >>> 8) & 0xff;
              state.check = crc32_1(state.check, hbuf, 2, 0);
              //===//
            }
            //=== INITBITS();
            hold = 0;
            bits = 0;
            //===//
            state.mode = EXLEN;
            /* falls through */
          case EXLEN:
            if (state.flags & 0x0400) {
              //=== NEEDBITS(16); */
              while (bits < 16) {
                if (have === 0) { break inf_leave; }
                have--;
                hold += input[next++] << bits;
                bits += 8;
              }
              //===//
              state.length = hold;
              if (state.head) {
                state.head.extra_len = hold;
              }
              if ((state.flags & 0x0200) && (state.wrap & 4)) {
                //=== CRC2(state.check, hold);
                hbuf[0] = hold & 0xff;
                hbuf[1] = (hold >>> 8) & 0xff;
                state.check = crc32_1(state.check, hbuf, 2, 0);
                //===//
              }
              //=== INITBITS();
              hold = 0;
              bits = 0;
              //===//
            }
            else if (state.head) {
              state.head.extra = null/*Z_NULL*/;
            }
            state.mode = EXTRA;
            /* falls through */
          case EXTRA:
            if (state.flags & 0x0400) {
              copy = state.length;
              if (copy > have) { copy = have; }
              if (copy) {
                if (state.head) {
                  len = state.head.extra_len - state.length;
                  if (!state.head.extra) {
                    // Use untyped array for more convenient processing later
                    state.head.extra = new Uint8Array(state.head.extra_len);
                  }
                  state.head.extra.set(
                    input.subarray(
                      next,
                      // extra field is limited to 65536 bytes
                      // - no need for additional size check
                      next + copy
                    ),
                    /*len + copy > state.head.extra_max - len ? state.head.extra_max : copy,*/
                    len
                  );
                  //zmemcpy(state.head.extra + len, next,
                  //        len + copy > state.head.extra_max ?
                  //        state.head.extra_max - len : copy);
                }
                if ((state.flags & 0x0200) && (state.wrap & 4)) {
                  state.check = crc32_1(state.check, input, copy, next);
                }
                have -= copy;
                next += copy;
                state.length -= copy;
              }
              if (state.length) { break inf_leave; }
            }
            state.length = 0;
            state.mode = NAME;
            /* falls through */
          case NAME:
            if (state.flags & 0x0800) {
              if (have === 0) { break inf_leave; }
              copy = 0;
              do {
                // TODO: 2 or 1 bytes?
                len = input[next + copy++];
                /* use constant limit because in js we should not preallocate memory */
                if (state.head && len &&
                    (state.length < 65536 /*state.head.name_max*/)) {
                  state.head.name += String.fromCharCode(len);
                }
              } while (len && copy < have);

              if ((state.flags & 0x0200) && (state.wrap & 4)) {
                state.check = crc32_1(state.check, input, copy, next);
              }
              have -= copy;
              next += copy;
              if (len) { break inf_leave; }
            }
            else if (state.head) {
              state.head.name = null;
            }
            state.length = 0;
            state.mode = COMMENT;
            /* falls through */
          case COMMENT:
            if (state.flags & 0x1000) {
              if (have === 0) { break inf_leave; }
              copy = 0;
              do {
                len = input[next + copy++];
                /* use constant limit because in js we should not preallocate memory */
                if (state.head && len &&
                    (state.length < 65536 /*state.head.comm_max*/)) {
                  state.head.comment += String.fromCharCode(len);
                }
              } while (len && copy < have);
              if ((state.flags & 0x0200) && (state.wrap & 4)) {
                state.check = crc32_1(state.check, input, copy, next);
              }
              have -= copy;
              next += copy;
              if (len) { break inf_leave; }
            }
            else if (state.head) {
              state.head.comment = null;
            }
            state.mode = HCRC;
            /* falls through */
          case HCRC:
            if (state.flags & 0x0200) {
              //=== NEEDBITS(16); */
              while (bits < 16) {
                if (have === 0) { break inf_leave; }
                have--;
                hold += input[next++] << bits;
                bits += 8;
              }
              //===//
              if ((state.wrap & 4) && hold !== (state.check & 0xffff)) {
                strm.msg = 'header crc mismatch';
                state.mode = BAD;
                break;
              }
              //=== INITBITS();
              hold = 0;
              bits = 0;
              //===//
            }
            if (state.head) {
              state.head.hcrc = ((state.flags >> 9) & 1);
              state.head.done = true;
            }
            strm.adler = state.check = 0;
            state.mode = TYPE;
            break;
          case DICTID:
            //=== NEEDBITS(32); */
            while (bits < 32) {
              if (have === 0) { break inf_leave; }
              have--;
              hold += input[next++] << bits;
              bits += 8;
            }
            //===//
            strm.adler = state.check = zswap32(hold);
            //=== INITBITS();
            hold = 0;
            bits = 0;
            //===//
            state.mode = DICT;
            /* falls through */
          case DICT:
            if (state.havedict === 0) {
              //--- RESTORE() ---
              strm.next_out = put;
              strm.avail_out = left;
              strm.next_in = next;
              strm.avail_in = have;
              state.hold = hold;
              state.bits = bits;
              //---
              return Z_NEED_DICT$1;
            }
            strm.adler = state.check = 1/*adler32(0L, Z_NULL, 0)*/;
            state.mode = TYPE;
            /* falls through */
          case TYPE:
            if (flush === Z_BLOCK || flush === Z_TREES) { break inf_leave; }
            /* falls through */
          case TYPEDO:
            if (state.last) {
              //--- BYTEBITS() ---//
              hold >>>= bits & 7;
              bits -= bits & 7;
              //---//
              state.mode = CHECK;
              break;
            }
            //=== NEEDBITS(3); */
            while (bits < 3) {
              if (have === 0) { break inf_leave; }
              have--;
              hold += input[next++] << bits;
              bits += 8;
            }
            //===//
            state.last = (hold & 0x01)/*BITS(1)*/;
            //--- DROPBITS(1) ---//
            hold >>>= 1;
            bits -= 1;
            //---//

            switch ((hold & 0x03)/*BITS(2)*/) {
              case 0:                             /* stored block */
                //Tracev((stderr, "inflate:     stored block%s\n",
                //        state.last ? " (last)" : ""));
                state.mode = STORED;
                break;
              case 1:                             /* fixed block */
                fixedtables(state);
                //Tracev((stderr, "inflate:     fixed codes block%s\n",
                //        state.last ? " (last)" : ""));
                state.mode = LEN_;             /* decode codes */
                if (flush === Z_TREES) {
                  //--- DROPBITS(2) ---//
                  hold >>>= 2;
                  bits -= 2;
                  //---//
                  break inf_leave;
                }
                break;
              case 2:                             /* dynamic block */
                //Tracev((stderr, "inflate:     dynamic codes block%s\n",
                //        state.last ? " (last)" : ""));
                state.mode = TABLE;
                break;
              case 3:
                strm.msg = 'invalid block type';
                state.mode = BAD;
            }
            //--- DROPBITS(2) ---//
            hold >>>= 2;
            bits -= 2;
            //---//
            break;
          case STORED:
            //--- BYTEBITS() ---// /* go to byte boundary */
            hold >>>= bits & 7;
            bits -= bits & 7;
            //---//
            //=== NEEDBITS(32); */
            while (bits < 32) {
              if (have === 0) { break inf_leave; }
              have--;
              hold += input[next++] << bits;
              bits += 8;
            }
            //===//
            if ((hold & 0xffff) !== ((hold >>> 16) ^ 0xffff)) {
              strm.msg = 'invalid stored block lengths';
              state.mode = BAD;
              break;
            }
            state.length = hold & 0xffff;
            //Tracev((stderr, "inflate:       stored length %u\n",
            //        state.length));
            //=== INITBITS();
            hold = 0;
            bits = 0;
            //===//
            state.mode = COPY_;
            if (flush === Z_TREES) { break inf_leave; }
            /* falls through */
          case COPY_:
            state.mode = COPY;
            /* falls through */
          case COPY:
            copy = state.length;
            if (copy) {
              if (copy > have) { copy = have; }
              if (copy > left) { copy = left; }
              if (copy === 0) { break inf_leave; }
              //--- zmemcpy(put, next, copy); ---
              output.set(input.subarray(next, next + copy), put);
              //---//
              have -= copy;
              next += copy;
              left -= copy;
              put += copy;
              state.length -= copy;
              break;
            }
            //Tracev((stderr, "inflate:       stored end\n"));
            state.mode = TYPE;
            break;
          case TABLE:
            //=== NEEDBITS(14); */
            while (bits < 14) {
              if (have === 0) { break inf_leave; }
              have--;
              hold += input[next++] << bits;
              bits += 8;
            }
            //===//
            state.nlen = (hold & 0x1f)/*BITS(5)*/ + 257;
            //--- DROPBITS(5) ---//
            hold >>>= 5;
            bits -= 5;
            //---//
            state.ndist = (hold & 0x1f)/*BITS(5)*/ + 1;
            //--- DROPBITS(5) ---//
            hold >>>= 5;
            bits -= 5;
            //---//
            state.ncode = (hold & 0x0f)/*BITS(4)*/ + 4;
            //--- DROPBITS(4) ---//
            hold >>>= 4;
            bits -= 4;
            //---//
    //#ifndef PKZIP_BUG_WORKAROUND
            if (state.nlen > 286 || state.ndist > 30) {
              strm.msg = 'too many length or distance symbols';
              state.mode = BAD;
              break;
            }
    //#endif
            //Tracev((stderr, "inflate:       table sizes ok\n"));
            state.have = 0;
            state.mode = LENLENS;
            /* falls through */
          case LENLENS:
            while (state.have < state.ncode) {
              //=== NEEDBITS(3);
              while (bits < 3) {
                if (have === 0) { break inf_leave; }
                have--;
                hold += input[next++] << bits;
                bits += 8;
              }
              //===//
              state.lens[order[state.have++]] = (hold & 0x07);//BITS(3);
              //--- DROPBITS(3) ---//
              hold >>>= 3;
              bits -= 3;
              //---//
            }
            while (state.have < 19) {
              state.lens[order[state.have++]] = 0;
            }
            // We have separate tables & no pointers. 2 commented lines below not needed.
            //state.next = state.codes;
            //state.lencode = state.next;
            // Switch to use dynamic table
            state.lencode = state.lendyn;
            state.lenbits = 7;

            opts = { bits: state.lenbits };
            ret = inftrees(CODES, state.lens, 0, 19, state.lencode, 0, state.work, opts);
            state.lenbits = opts.bits;

            if (ret) {
              strm.msg = 'invalid code lengths set';
              state.mode = BAD;
              break;
            }
            //Tracev((stderr, "inflate:       code lengths ok\n"));
            state.have = 0;
            state.mode = CODELENS;
            /* falls through */
          case CODELENS:
            while (state.have < state.nlen + state.ndist) {
              for (;;) {
                here = state.lencode[hold & ((1 << state.lenbits) - 1)];/*BITS(state.lenbits)*/
                here_bits = here >>> 24;
                here_op = (here >>> 16) & 0xff;
                here_val = here & 0xffff;

                if ((here_bits) <= bits) { break; }
                //--- PULLBYTE() ---//
                if (have === 0) { break inf_leave; }
                have--;
                hold += input[next++] << bits;
                bits += 8;
                //---//
              }
              if (here_val < 16) {
                //--- DROPBITS(here.bits) ---//
                hold >>>= here_bits;
                bits -= here_bits;
                //---//
                state.lens[state.have++] = here_val;
              }
              else {
                if (here_val === 16) {
                  //=== NEEDBITS(here.bits + 2);
                  n = here_bits + 2;
                  while (bits < n) {
                    if (have === 0) { break inf_leave; }
                    have--;
                    hold += input[next++] << bits;
                    bits += 8;
                  }
                  //===//
                  //--- DROPBITS(here.bits) ---//
                  hold >>>= here_bits;
                  bits -= here_bits;
                  //---//
                  if (state.have === 0) {
                    strm.msg = 'invalid bit length repeat';
                    state.mode = BAD;
                    break;
                  }
                  len = state.lens[state.have - 1];
                  copy = 3 + (hold & 0x03);//BITS(2);
                  //--- DROPBITS(2) ---//
                  hold >>>= 2;
                  bits -= 2;
                  //---//
                }
                else if (here_val === 17) {
                  //=== NEEDBITS(here.bits + 3);
                  n = here_bits + 3;
                  while (bits < n) {
                    if (have === 0) { break inf_leave; }
                    have--;
                    hold += input[next++] << bits;
                    bits += 8;
                  }
                  //===//
                  //--- DROPBITS(here.bits) ---//
                  hold >>>= here_bits;
                  bits -= here_bits;
                  //---//
                  len = 0;
                  copy = 3 + (hold & 0x07);//BITS(3);
                  //--- DROPBITS(3) ---//
                  hold >>>= 3;
                  bits -= 3;
                  //---//
                }
                else {
                  //=== NEEDBITS(here.bits + 7);
                  n = here_bits + 7;
                  while (bits < n) {
                    if (have === 0) { break inf_leave; }
                    have--;
                    hold += input[next++] << bits;
                    bits += 8;
                  }
                  //===//
                  //--- DROPBITS(here.bits) ---//
                  hold >>>= here_bits;
                  bits -= here_bits;
                  //---//
                  len = 0;
                  copy = 11 + (hold & 0x7f);//BITS(7);
                  //--- DROPBITS(7) ---//
                  hold >>>= 7;
                  bits -= 7;
                  //---//
                }
                if (state.have + copy > state.nlen + state.ndist) {
                  strm.msg = 'invalid bit length repeat';
                  state.mode = BAD;
                  break;
                }
                while (copy--) {
                  state.lens[state.have++] = len;
                }
              }
            }

            /* handle error breaks in while */
            if (state.mode === BAD) { break; }

            /* check for end-of-block code (better have one) */
            if (state.lens[256] === 0) {
              strm.msg = 'invalid code -- missing end-of-block';
              state.mode = BAD;
              break;
            }

            /* build code tables -- note: do not change the lenbits or distbits
               values here (9 and 6) without reading the comments in inftrees.h
               concerning the ENOUGH constants, which depend on those values */
            state.lenbits = 9;

            opts = { bits: state.lenbits };
            ret = inftrees(LENS, state.lens, 0, state.nlen, state.lencode, 0, state.work, opts);
            // We have separate tables & no pointers. 2 commented lines below not needed.
            // state.next_index = opts.table_index;
            state.lenbits = opts.bits;
            // state.lencode = state.next;

            if (ret) {
              strm.msg = 'invalid literal/lengths set';
              state.mode = BAD;
              break;
            }

            state.distbits = 6;
            //state.distcode.copy(state.codes);
            // Switch to use dynamic table
            state.distcode = state.distdyn;
            opts = { bits: state.distbits };
            ret = inftrees(DISTS, state.lens, state.nlen, state.ndist, state.distcode, 0, state.work, opts);
            // We have separate tables & no pointers. 2 commented lines below not needed.
            // state.next_index = opts.table_index;
            state.distbits = opts.bits;
            // state.distcode = state.next;

            if (ret) {
              strm.msg = 'invalid distances set';
              state.mode = BAD;
              break;
            }
            //Tracev((stderr, 'inflate:       codes ok\n'));
            state.mode = LEN_;
            if (flush === Z_TREES) { break inf_leave; }
            /* falls through */
          case LEN_:
            state.mode = LEN;
            /* falls through */
          case LEN:
            if (have >= 6 && left >= 258) {
              //--- RESTORE() ---
              strm.next_out = put;
              strm.avail_out = left;
              strm.next_in = next;
              strm.avail_in = have;
              state.hold = hold;
              state.bits = bits;
              //---
              inffast(strm, _out);
              //--- LOAD() ---
              put = strm.next_out;
              output = strm.output;
              left = strm.avail_out;
              next = strm.next_in;
              input = strm.input;
              have = strm.avail_in;
              hold = state.hold;
              bits = state.bits;
              //---

              if (state.mode === TYPE) {
                state.back = -1;
              }
              break;
            }
            state.back = 0;
            for (;;) {
              here = state.lencode[hold & ((1 << state.lenbits) - 1)];  /*BITS(state.lenbits)*/
              here_bits = here >>> 24;
              here_op = (here >>> 16) & 0xff;
              here_val = here & 0xffff;

              if (here_bits <= bits) { break; }
              //--- PULLBYTE() ---//
              if (have === 0) { break inf_leave; }
              have--;
              hold += input[next++] << bits;
              bits += 8;
              //---//
            }
            if (here_op && (here_op & 0xf0) === 0) {
              last_bits = here_bits;
              last_op = here_op;
              last_val = here_val;
              for (;;) {
                here = state.lencode[last_val +
                        ((hold & ((1 << (last_bits + last_op)) - 1))/*BITS(last.bits + last.op)*/ >> last_bits)];
                here_bits = here >>> 24;
                here_op = (here >>> 16) & 0xff;
                here_val = here & 0xffff;

                if ((last_bits + here_bits) <= bits) { break; }
                //--- PULLBYTE() ---//
                if (have === 0) { break inf_leave; }
                have--;
                hold += input[next++] << bits;
                bits += 8;
                //---//
              }
              //--- DROPBITS(last.bits) ---//
              hold >>>= last_bits;
              bits -= last_bits;
              //---//
              state.back += last_bits;
            }
            //--- DROPBITS(here.bits) ---//
            hold >>>= here_bits;
            bits -= here_bits;
            //---//
            state.back += here_bits;
            state.length = here_val;
            if (here_op === 0) {
              //Tracevv((stderr, here.val >= 0x20 && here.val < 0x7f ?
              //        "inflate:         literal '%c'\n" :
              //        "inflate:         literal 0x%02x\n", here.val));
              state.mode = LIT;
              break;
            }
            if (here_op & 32) {
              //Tracevv((stderr, "inflate:         end of block\n"));
              state.back = -1;
              state.mode = TYPE;
              break;
            }
            if (here_op & 64) {
              strm.msg = 'invalid literal/length code';
              state.mode = BAD;
              break;
            }
            state.extra = here_op & 15;
            state.mode = LENEXT;
            /* falls through */
          case LENEXT:
            if (state.extra) {
              //=== NEEDBITS(state.extra);
              n = state.extra;
              while (bits < n) {
                if (have === 0) { break inf_leave; }
                have--;
                hold += input[next++] << bits;
                bits += 8;
              }
              //===//
              state.length += hold & ((1 << state.extra) - 1)/*BITS(state.extra)*/;
              //--- DROPBITS(state.extra) ---//
              hold >>>= state.extra;
              bits -= state.extra;
              //---//
              state.back += state.extra;
            }
            //Tracevv((stderr, "inflate:         length %u\n", state.length));
            state.was = state.length;
            state.mode = DIST;
            /* falls through */
          case DIST:
            for (;;) {
              here = state.distcode[hold & ((1 << state.distbits) - 1)];/*BITS(state.distbits)*/
              here_bits = here >>> 24;
              here_op = (here >>> 16) & 0xff;
              here_val = here & 0xffff;

              if ((here_bits) <= bits) { break; }
              //--- PULLBYTE() ---//
              if (have === 0) { break inf_leave; }
              have--;
              hold += input[next++] << bits;
              bits += 8;
              //---//
            }
            if ((here_op & 0xf0) === 0) {
              last_bits = here_bits;
              last_op = here_op;
              last_val = here_val;
              for (;;) {
                here = state.distcode[last_val +
                        ((hold & ((1 << (last_bits + last_op)) - 1))/*BITS(last.bits + last.op)*/ >> last_bits)];
                here_bits = here >>> 24;
                here_op = (here >>> 16) & 0xff;
                here_val = here & 0xffff;

                if ((last_bits + here_bits) <= bits) { break; }
                //--- PULLBYTE() ---//
                if (have === 0) { break inf_leave; }
                have--;
                hold += input[next++] << bits;
                bits += 8;
                //---//
              }
              //--- DROPBITS(last.bits) ---//
              hold >>>= last_bits;
              bits -= last_bits;
              //---//
              state.back += last_bits;
            }
            //--- DROPBITS(here.bits) ---//
            hold >>>= here_bits;
            bits -= here_bits;
            //---//
            state.back += here_bits;
            if (here_op & 64) {
              strm.msg = 'invalid distance code';
              state.mode = BAD;
              break;
            }
            state.offset = here_val;
            state.extra = (here_op) & 15;
            state.mode = DISTEXT;
            /* falls through */
          case DISTEXT:
            if (state.extra) {
              //=== NEEDBITS(state.extra);
              n = state.extra;
              while (bits < n) {
                if (have === 0) { break inf_leave; }
                have--;
                hold += input[next++] << bits;
                bits += 8;
              }
              //===//
              state.offset += hold & ((1 << state.extra) - 1)/*BITS(state.extra)*/;
              //--- DROPBITS(state.extra) ---//
              hold >>>= state.extra;
              bits -= state.extra;
              //---//
              state.back += state.extra;
            }
    //#ifdef INFLATE_STRICT
            if (state.offset > state.dmax) {
              strm.msg = 'invalid distance too far back';
              state.mode = BAD;
              break;
            }
    //#endif
            //Tracevv((stderr, "inflate:         distance %u\n", state.offset));
            state.mode = MATCH;
            /* falls through */
          case MATCH:
            if (left === 0) { break inf_leave; }
            copy = _out - left;
            if (state.offset > copy) {         /* copy from window */
              copy = state.offset - copy;
              if (copy > state.whave) {
                if (state.sane) {
                  strm.msg = 'invalid distance too far back';
                  state.mode = BAD;
                  break;
                }
    // (!) This block is disabled in zlib defaults,
    // don't enable it for binary compatibility
    //#ifdef INFLATE_ALLOW_INVALID_DISTANCE_TOOFAR_ARRR
    //          Trace((stderr, "inflate.c too far\n"));
    //          copy -= state.whave;
    //          if (copy > state.length) { copy = state.length; }
    //          if (copy > left) { copy = left; }
    //          left -= copy;
    //          state.length -= copy;
    //          do {
    //            output[put++] = 0;
    //          } while (--copy);
    //          if (state.length === 0) { state.mode = LEN; }
    //          break;
    //#endif
              }
              if (copy > state.wnext) {
                copy -= state.wnext;
                from = state.wsize - copy;
              }
              else {
                from = state.wnext - copy;
              }
              if (copy > state.length) { copy = state.length; }
              from_source = state.window;
            }
            else {                              /* copy from output */
              from_source = output;
              from = put - state.offset;
              copy = state.length;
            }
            if (copy > left) { copy = left; }
            left -= copy;
            state.length -= copy;
            do {
              output[put++] = from_source[from++];
            } while (--copy);
            if (state.length === 0) { state.mode = LEN; }
            break;
          case LIT:
            if (left === 0) { break inf_leave; }
            output[put++] = state.length;
            left--;
            state.mode = LEN;
            break;
          case CHECK:
            if (state.wrap) {
              //=== NEEDBITS(32);
              while (bits < 32) {
                if (have === 0) { break inf_leave; }
                have--;
                // Use '|' instead of '+' to make sure that result is signed
                hold |= input[next++] << bits;
                bits += 8;
              }
              //===//
              _out -= left;
              strm.total_out += _out;
              state.total += _out;
              if ((state.wrap & 4) && _out) {
                strm.adler = state.check =
                    /*UPDATE_CHECK(state.check, put - _out, _out);*/
                    (state.flags ? crc32_1(state.check, output, _out, put - _out) : adler32_1(state.check, output, _out, put - _out));

              }
              _out = left;
              // NB: crc32 stored as signed 32-bit int, zswap32 returns signed too
              if ((state.wrap & 4) && (state.flags ? hold : zswap32(hold)) !== state.check) {
                strm.msg = 'incorrect data check';
                state.mode = BAD;
                break;
              }
              //=== INITBITS();
              hold = 0;
              bits = 0;
              //===//
              //Tracev((stderr, "inflate:   check matches trailer\n"));
            }
            state.mode = LENGTH;
            /* falls through */
          case LENGTH:
            if (state.wrap && state.flags) {
              //=== NEEDBITS(32);
              while (bits < 32) {
                if (have === 0) { break inf_leave; }
                have--;
                hold += input[next++] << bits;
                bits += 8;
              }
              //===//
              if ((state.wrap & 4) && hold !== (state.total & 0xffffffff)) {
                strm.msg = 'incorrect length check';
                state.mode = BAD;
                break;
              }
              //=== INITBITS();
              hold = 0;
              bits = 0;
              //===//
              //Tracev((stderr, "inflate:   length matches trailer\n"));
            }
            state.mode = DONE;
            /* falls through */
          case DONE:
            ret = Z_STREAM_END$1;
            break inf_leave;
          case BAD:
            ret = Z_DATA_ERROR$1;
            break inf_leave;
          case MEM:
            return Z_MEM_ERROR$1;
          case SYNC:
            /* falls through */
          default:
            return Z_STREAM_ERROR$1;
        }
      }

      // inf_leave <- here is real place for "goto inf_leave", emulated via "break inf_leave"

      /*
         Return from inflate(), updating the total counts and the check value.
         If there was no progress during the inflate() call, return a buffer
         error.  Call updatewindow() to create and/or update the window state.
         Note: a memory error from inflate() is non-recoverable.
       */

      //--- RESTORE() ---
      strm.next_out = put;
      strm.avail_out = left;
      strm.next_in = next;
      strm.avail_in = have;
      state.hold = hold;
      state.bits = bits;
      //---

      if (state.wsize || (_out !== strm.avail_out && state.mode < BAD &&
                          (state.mode < CHECK || flush !== Z_FINISH$1))) {
        if (updatewindow(strm, strm.output, strm.next_out, _out - strm.avail_out)) ;
      }
      _in -= strm.avail_in;
      _out -= strm.avail_out;
      strm.total_in += _in;
      strm.total_out += _out;
      state.total += _out;
      if ((state.wrap & 4) && _out) {
        strm.adler = state.check = /*UPDATE_CHECK(state.check, strm.next_out - _out, _out);*/
          (state.flags ? crc32_1(state.check, output, _out, strm.next_out - _out) : adler32_1(state.check, output, _out, strm.next_out - _out));
      }
      strm.data_type = state.bits + (state.last ? 64 : 0) +
                        (state.mode === TYPE ? 128 : 0) +
                        (state.mode === LEN_ || state.mode === COPY_ ? 256 : 0);
      if (((_in === 0 && _out === 0) || flush === Z_FINISH$1) && ret === Z_OK$1) {
        ret = Z_BUF_ERROR;
      }
      return ret;
    };


    const inflateEnd = (strm) => {

      if (inflateStateCheck(strm)) {
        return Z_STREAM_ERROR$1;
      }

      let state = strm.state;
      if (state.window) {
        state.window = null;
      }
      strm.state = null;
      return Z_OK$1;
    };


    const inflateGetHeader = (strm, head) => {

      /* check state */
      if (inflateStateCheck(strm)) { return Z_STREAM_ERROR$1; }
      const state = strm.state;
      if ((state.wrap & 2) === 0) { return Z_STREAM_ERROR$1; }

      /* save header structure */
      state.head = head;
      head.done = false;
      return Z_OK$1;
    };


    const inflateSetDictionary = (strm, dictionary) => {
      const dictLength = dictionary.length;

      let state;
      let dictid;
      let ret;

      /* check state */
      if (inflateStateCheck(strm)) { return Z_STREAM_ERROR$1; }
      state = strm.state;

      if (state.wrap !== 0 && state.mode !== DICT) {
        return Z_STREAM_ERROR$1;
      }

      /* check for correct dictionary identifier */
      if (state.mode === DICT) {
        dictid = 1; /* adler32(0, null, 0)*/
        /* dictid = adler32(dictid, dictionary, dictLength); */
        dictid = adler32_1(dictid, dictionary, dictLength, 0);
        if (dictid !== state.check) {
          return Z_DATA_ERROR$1;
        }
      }
      /* copy dictionary to window using updatewindow(), which will amend the
       existing dictionary if appropriate */
      ret = updatewindow(strm, dictionary, dictLength, dictLength);
      if (ret) {
        state.mode = MEM;
        return Z_MEM_ERROR$1;
      }
      state.havedict = 1;
      // Tracev((stderr, "inflate:   dictionary set\n"));
      return Z_OK$1;
    };


    var inflateReset_1 = inflateReset;
    var inflateReset2_1 = inflateReset2;
    var inflateResetKeep_1 = inflateResetKeep;
    var inflateInit_1 = inflateInit;
    var inflateInit2_1 = inflateInit2;
    var inflate_2$1 = inflate$2;
    var inflateEnd_1 = inflateEnd;
    var inflateGetHeader_1 = inflateGetHeader;
    var inflateSetDictionary_1 = inflateSetDictionary;
    var inflateInfo = 'pako inflate (from Nodeca project)';

    /* Not implemented
    module.exports.inflateCodesUsed = inflateCodesUsed;
    module.exports.inflateCopy = inflateCopy;
    module.exports.inflateGetDictionary = inflateGetDictionary;
    module.exports.inflateMark = inflateMark;
    module.exports.inflatePrime = inflatePrime;
    module.exports.inflateSync = inflateSync;
    module.exports.inflateSyncPoint = inflateSyncPoint;
    module.exports.inflateUndermine = inflateUndermine;
    module.exports.inflateValidate = inflateValidate;
    */

    var inflate_1$2 = {
    	inflateReset: inflateReset_1,
    	inflateReset2: inflateReset2_1,
    	inflateResetKeep: inflateResetKeep_1,
    	inflateInit: inflateInit_1,
    	inflateInit2: inflateInit2_1,
    	inflate: inflate_2$1,
    	inflateEnd: inflateEnd_1,
    	inflateGetHeader: inflateGetHeader_1,
    	inflateSetDictionary: inflateSetDictionary_1,
    	inflateInfo: inflateInfo
    };

    // (C) 1995-2013 Jean-loup Gailly and Mark Adler
    // (C) 2014-2017 Vitaly Puzrin and Andrey Tupitsin
    //
    // This software is provided 'as-is', without any express or implied
    // warranty. In no event will the authors be held liable for any damages
    // arising from the use of this software.
    //
    // Permission is granted to anyone to use this software for any purpose,
    // including commercial applications, and to alter it and redistribute it
    // freely, subject to the following restrictions:
    //
    // 1. The origin of this software must not be misrepresented; you must not
    //   claim that you wrote the original software. If you use this software
    //   in a product, an acknowledgment in the product documentation would be
    //   appreciated but is not required.
    // 2. Altered source versions must be plainly marked as such, and must not be
    //   misrepresented as being the original software.
    // 3. This notice may not be removed or altered from any source distribution.

    function GZheader() {
      /* true if compressed data believed to be text */
      this.text       = 0;
      /* modification time */
      this.time       = 0;
      /* extra flags (not used when writing a gzip file) */
      this.xflags     = 0;
      /* operating system */
      this.os         = 0;
      /* pointer to extra field or Z_NULL if none */
      this.extra      = null;
      /* extra field length (valid if extra != Z_NULL) */
      this.extra_len  = 0; // Actually, we don't need it in JS,
                           // but leave for few code modifications

      //
      // Setup limits is not necessary because in js we should not preallocate memory
      // for inflate use constant limit in 65536 bytes
      //

      /* space at extra (only when reading header) */
      // this.extra_max  = 0;
      /* pointer to zero-terminated file name or Z_NULL */
      this.name       = '';
      /* space at name (only when reading header) */
      // this.name_max   = 0;
      /* pointer to zero-terminated comment or Z_NULL */
      this.comment    = '';
      /* space at comment (only when reading header) */
      // this.comm_max   = 0;
      /* true if there was or will be a header crc */
      this.hcrc       = 0;
      /* true when done reading gzip header (not used when writing a gzip file) */
      this.done       = false;
    }

    var gzheader = GZheader;

    const toString = Object.prototype.toString;

    /* Public constants ==========================================================*/
    /* ===========================================================================*/

    const {
      Z_NO_FLUSH, Z_FINISH,
      Z_OK, Z_STREAM_END, Z_NEED_DICT, Z_STREAM_ERROR, Z_DATA_ERROR, Z_MEM_ERROR
    } = constants$2;

    /* ===========================================================================*/


    /**
     * class Inflate
     *
     * Generic JS-style wrapper for zlib calls. If you don't need
     * streaming behaviour - use more simple functions: [[inflate]]
     * and [[inflateRaw]].
     **/

    /* internal
     * inflate.chunks -> Array
     *
     * Chunks of output data, if [[Inflate#onData]] not overridden.
     **/

    /**
     * Inflate.result -> Uint8Array|String
     *
     * Uncompressed result, generated by default [[Inflate#onData]]
     * and [[Inflate#onEnd]] handlers. Filled after you push last chunk
     * (call [[Inflate#push]] with `Z_FINISH` / `true` param).
     **/

    /**
     * Inflate.err -> Number
     *
     * Error code after inflate finished. 0 (Z_OK) on success.
     * Should be checked if broken data possible.
     **/

    /**
     * Inflate.msg -> String
     *
     * Error message, if [[Inflate.err]] != 0
     **/


    /**
     * new Inflate(options)
     * - options (Object): zlib inflate options.
     *
     * Creates new inflator instance with specified params. Throws exception
     * on bad params. Supported options:
     *
     * - `windowBits`
     * - `dictionary`
     *
     * [http://zlib.net/manual.html#Advanced](http://zlib.net/manual.html#Advanced)
     * for more information on these.
     *
     * Additional options, for internal needs:
     *
     * - `chunkSize` - size of generated data chunks (16K by default)
     * - `raw` (Boolean) - do raw inflate
     * - `to` (String) - if equal to 'string', then result will be converted
     *   from utf8 to utf16 (javascript) string. When string output requested,
     *   chunk length can differ from `chunkSize`, depending on content.
     *
     * By default, when no options set, autodetect deflate/gzip data format via
     * wrapper header.
     *
     * ##### Example:
     *
     * ```javascript
     * const pako = require('pako')
     * const chunk1 = new Uint8Array([1,2,3,4,5,6,7,8,9])
     * const chunk2 = new Uint8Array([10,11,12,13,14,15,16,17,18,19]);
     *
     * const inflate = new pako.Inflate({ level: 3});
     *
     * inflate.push(chunk1, false);
     * inflate.push(chunk2, true);  // true -> last chunk
     *
     * if (inflate.err) { throw new Error(inflate.err); }
     *
     * console.log(inflate.result);
     * ```
     **/
    function Inflate$1(options) {
      this.options = common.assign({
        chunkSize: 1024 * 64,
        windowBits: 15,
        to: ''
      }, options || {});

      const opt = this.options;

      // Force window size for `raw` data, if not set directly,
      // because we have no header for autodetect.
      if (opt.raw && (opt.windowBits >= 0) && (opt.windowBits < 16)) {
        opt.windowBits = -opt.windowBits;
        if (opt.windowBits === 0) { opt.windowBits = -15; }
      }

      // If `windowBits` not defined (and mode not raw) - set autodetect flag for gzip/deflate
      if ((opt.windowBits >= 0) && (opt.windowBits < 16) &&
          !(options && options.windowBits)) {
        opt.windowBits += 32;
      }

      // Gzip header has no info about windows size, we can do autodetect only
      // for deflate. So, if window size not set, force it to max when gzip possible
      if ((opt.windowBits > 15) && (opt.windowBits < 48)) {
        // bit 3 (16) -> gzipped data
        // bit 4 (32) -> autodetect gzip/deflate
        if ((opt.windowBits & 15) === 0) {
          opt.windowBits |= 15;
        }
      }

      this.err    = 0;      // error code, if happens (0 = Z_OK)
      this.msg    = '';     // error message
      this.ended  = false;  // used to avoid multiple onEnd() calls
      this.chunks = [];     // chunks of compressed data

      this.strm   = new zstream();
      this.strm.avail_out = 0;

      let status  = inflate_1$2.inflateInit2(
        this.strm,
        opt.windowBits
      );

      if (status !== Z_OK) {
        throw new Error(messages[status]);
      }

      this.header = new gzheader();

      inflate_1$2.inflateGetHeader(this.strm, this.header);

      // Setup dictionary
      if (opt.dictionary) {
        // Convert data if needed
        if (typeof opt.dictionary === 'string') {
          opt.dictionary = strings.string2buf(opt.dictionary);
        } else if (toString.call(opt.dictionary) === '[object ArrayBuffer]') {
          opt.dictionary = new Uint8Array(opt.dictionary);
        }
        if (opt.raw) { //In raw mode we need to set the dictionary early
          status = inflate_1$2.inflateSetDictionary(this.strm, opt.dictionary);
          if (status !== Z_OK) {
            throw new Error(messages[status]);
          }
        }
      }
    }

    /**
     * Inflate#push(data[, flush_mode]) -> Boolean
     * - data (Uint8Array|ArrayBuffer): input data
     * - flush_mode (Number|Boolean): 0..6 for corresponding Z_NO_FLUSH..Z_TREE
     *   flush modes. See constants. Skipped or `false` means Z_NO_FLUSH,
     *   `true` means Z_FINISH.
     *
     * Sends input data to inflate pipe, generating [[Inflate#onData]] calls with
     * new output chunks. Returns `true` on success. If end of stream detected,
     * [[Inflate#onEnd]] will be called.
     *
     * `flush_mode` is not needed for normal operation, because end of stream
     * detected automatically. You may try to use it for advanced things, but
     * this functionality was not tested.
     *
     * On fail call [[Inflate#onEnd]] with error code and return false.
     *
     * ##### Example
     *
     * ```javascript
     * push(chunk, false); // push one of data chunks
     * ...
     * push(chunk, true);  // push last chunk
     * ```
     **/
    Inflate$1.prototype.push = function (data, flush_mode) {
      const strm = this.strm;
      const chunkSize = this.options.chunkSize;
      const dictionary = this.options.dictionary;
      let status, _flush_mode, last_avail_out;

      if (this.ended) return false;

      if (flush_mode === ~~flush_mode) _flush_mode = flush_mode;
      else _flush_mode = flush_mode === true ? Z_FINISH : Z_NO_FLUSH;

      // Convert data if needed
      if (toString.call(data) === '[object ArrayBuffer]') {
        strm.input = new Uint8Array(data);
      } else {
        strm.input = data;
      }

      strm.next_in = 0;
      strm.avail_in = strm.input.length;

      for (;;) {
        if (strm.avail_out === 0) {
          strm.output = new Uint8Array(chunkSize);
          strm.next_out = 0;
          strm.avail_out = chunkSize;
        }

        status = inflate_1$2.inflate(strm, _flush_mode);

        if (status === Z_NEED_DICT && dictionary) {
          status = inflate_1$2.inflateSetDictionary(strm, dictionary);

          if (status === Z_OK) {
            status = inflate_1$2.inflate(strm, _flush_mode);
          } else if (status === Z_DATA_ERROR) {
            // Replace code with more verbose
            status = Z_NEED_DICT;
          }
        }

        // Skip snyc markers if more data follows and not raw mode
        while (strm.avail_in > 0 &&
               status === Z_STREAM_END &&
               strm.state.wrap > 0 &&
               data[strm.next_in] !== 0)
        {
          inflate_1$2.inflateReset(strm);
          status = inflate_1$2.inflate(strm, _flush_mode);
        }

        switch (status) {
          case Z_STREAM_ERROR:
          case Z_DATA_ERROR:
          case Z_NEED_DICT:
          case Z_MEM_ERROR:
            this.onEnd(status);
            this.ended = true;
            return false;
        }

        // Remember real `avail_out` value, because we may patch out buffer content
        // to align utf8 strings boundaries.
        last_avail_out = strm.avail_out;

        if (strm.next_out) {
          if (strm.avail_out === 0 || status === Z_STREAM_END) {

            if (this.options.to === 'string') {

              let next_out_utf8 = strings.utf8border(strm.output, strm.next_out);

              let tail = strm.next_out - next_out_utf8;
              let utf8str = strings.buf2string(strm.output, next_out_utf8);

              // move tail & realign counters
              strm.next_out = tail;
              strm.avail_out = chunkSize - tail;
              if (tail) strm.output.set(strm.output.subarray(next_out_utf8, next_out_utf8 + tail), 0);

              this.onData(utf8str);

            } else {
              this.onData(strm.output.length === strm.next_out ? strm.output : strm.output.subarray(0, strm.next_out));
            }
          }
        }

        // Must repeat iteration if out buffer is full
        if (status === Z_OK && last_avail_out === 0) continue;

        // Finalize if end of stream reached.
        if (status === Z_STREAM_END) {
          status = inflate_1$2.inflateEnd(this.strm);
          this.onEnd(status);
          this.ended = true;
          return true;
        }

        if (strm.avail_in === 0) break;
      }

      return true;
    };


    /**
     * Inflate#onData(chunk) -> Void
     * - chunk (Uint8Array|String): output data. When string output requested,
     *   each chunk will be string.
     *
     * By default, stores data blocks in `chunks[]` property and glue
     * those in `onEnd`. Override this handler, if you need another behaviour.
     **/
    Inflate$1.prototype.onData = function (chunk) {
      this.chunks.push(chunk);
    };


    /**
     * Inflate#onEnd(status) -> Void
     * - status (Number): inflate status. 0 (Z_OK) on success,
     *   other if not.
     *
     * Called either after you tell inflate that the input stream is
     * complete (Z_FINISH). By default - join collected chunks,
     * free memory and fill `results` / `err` properties.
     **/
    Inflate$1.prototype.onEnd = function (status) {
      // On success - join
      if (status === Z_OK) {
        if (this.options.to === 'string') {
          this.result = this.chunks.join('');
        } else {
          this.result = common.flattenChunks(this.chunks);
        }
      }
      this.chunks = [];
      this.err = status;
      this.msg = this.strm.msg;
    };


    /**
     * inflate(data[, options]) -> Uint8Array|String
     * - data (Uint8Array|ArrayBuffer): input data to decompress.
     * - options (Object): zlib inflate options.
     *
     * Decompress `data` with inflate/ungzip and `options`. Autodetect
     * format via wrapper header by default. That's why we don't provide
     * separate `ungzip` method.
     *
     * Supported options are:
     *
     * - windowBits
     *
     * [http://zlib.net/manual.html#Advanced](http://zlib.net/manual.html#Advanced)
     * for more information.
     *
     * Sugar (options):
     *
     * - `raw` (Boolean) - say that we work with raw stream, if you don't wish to specify
     *   negative windowBits implicitly.
     * - `to` (String) - if equal to 'string', then result will be converted
     *   from utf8 to utf16 (javascript) string. When string output requested,
     *   chunk length can differ from `chunkSize`, depending on content.
     *
     *
     * ##### Example:
     *
     * ```javascript
     * const pako = require('pako');
     * const input = pako.deflate(new Uint8Array([1,2,3,4,5,6,7,8,9]));
     * let output;
     *
     * try {
     *   output = pako.inflate(input);
     * } catch (err) {
     *   console.log(err);
     * }
     * ```
     **/
    function inflate$1(input, options) {
      const inflator = new Inflate$1(options);

      inflator.push(input);

      // That will never happens, if you don't cheat with options :)
      if (inflator.err) throw inflator.msg || messages[inflator.err];

      return inflator.result;
    }


    /**
     * inflateRaw(data[, options]) -> Uint8Array|String
     * - data (Uint8Array|ArrayBuffer): input data to decompress.
     * - options (Object): zlib inflate options.
     *
     * The same as [[inflate]], but creates raw data, without wrapper
     * (header and adler32 crc).
     **/
    function inflateRaw$1(input, options) {
      options = options || {};
      options.raw = true;
      return inflate$1(input, options);
    }


    /**
     * ungzip(data[, options]) -> Uint8Array|String
     * - data (Uint8Array|ArrayBuffer): input data to decompress.
     * - options (Object): zlib inflate options.
     *
     * Just shortcut to [[inflate]], because it autodetects format
     * by header.content. Done for convenience.
     **/


    var Inflate_1$1 = Inflate$1;
    var inflate_2 = inflate$1;
    var inflateRaw_1$1 = inflateRaw$1;
    var ungzip$1 = inflate$1;
    var constants = constants$2;

    var inflate_1$1 = {
    	Inflate: Inflate_1$1,
    	inflate: inflate_2,
    	inflateRaw: inflateRaw_1$1,
    	ungzip: ungzip$1,
    	constants: constants
    };

    const { Deflate, deflate, deflateRaw, gzip } = deflate_1$1;

    const { Inflate, inflate, inflateRaw, ungzip } = inflate_1$1;



    var Deflate_1 = Deflate;
    var deflate_1 = deflate;
    var deflateRaw_1 = deflateRaw;
    var gzip_1 = gzip;
    var Inflate_1 = Inflate;
    var inflate_1 = inflate;
    var inflateRaw_1 = inflateRaw;
    var ungzip_1 = ungzip;
    var constants_1 = constants$2;

    var pako = {
    	Deflate: Deflate_1,
    	deflate: deflate_1,
    	deflateRaw: deflateRaw_1,
    	gzip: gzip_1,
    	Inflate: Inflate_1,
    	inflate: inflate_1,
    	inflateRaw: inflateRaw_1,
    	ungzip: ungzip_1,
    	constants: constants_1
    };

    var NbtType;
    (function (NbtType) {
        NbtType[NbtType["End"] = 0] = "End";
        NbtType[NbtType["Byte"] = 1] = "Byte";
        NbtType[NbtType["Short"] = 2] = "Short";
        NbtType[NbtType["Int"] = 3] = "Int";
        NbtType[NbtType["Long"] = 4] = "Long";
        NbtType[NbtType["Float"] = 5] = "Float";
        NbtType[NbtType["Double"] = 6] = "Double";
        NbtType[NbtType["ByteArray"] = 7] = "ByteArray";
        NbtType[NbtType["String"] = 8] = "String";
        NbtType[NbtType["List"] = 9] = "List";
        NbtType[NbtType["Compound"] = 10] = "Compound";
        NbtType[NbtType["IntArray"] = 11] = "IntArray";
        NbtType[NbtType["LongArray"] = 12] = "LongArray";
    })(NbtType || (NbtType = {}));

    class NbtTag {
        static FACTORIES = new Map();
        static register(type, factory) {
            const factoryType = factory.create().getId();
            if (factoryType !== type) {
                throw new Error(`Registered factory ${NbtType[factoryType]} does not match type ${NbtType[type]}`);
            }
            NbtTag.FACTORIES.set(type, factory);
        }
        isEnd() {
            return this.getId() === NbtType.End;
        }
        isByte() {
            return this.getId() === NbtType.Byte;
        }
        isShort() {
            return this.getId() === NbtType.Short;
        }
        isInt() {
            return this.getId() === NbtType.Int;
        }
        isLong() {
            return this.getId() === NbtType.Long;
        }
        isFloat() {
            return this.getId() === NbtType.Float;
        }
        isDouble() {
            return this.getId() === NbtType.Double;
        }
        isByteArray() {
            return this.getId() === NbtType.ByteArray;
        }
        isString() {
            return this.getId() === NbtType.String;
        }
        isList() {
            return this.getId() === NbtType.List;
        }
        isCompound() {
            return this.getId() === NbtType.Compound;
        }
        isIntArray() {
            return this.getId() === NbtType.IntArray;
        }
        isLongArray() {
            return this.getId() === NbtType.LongArray;
        }
        isNumber() {
            return this.isByte() || this.isShort() || this.isInt() || this.isLong() || this.isFloat() || this.isDouble();
        }
        isArray() {
            return this.isByteArray() || this.isIntArray() || this.isLongArray();
        }
        isListOrArray() {
            return this.isList() || this.isArray();
        }
        getAsNumber() {
            return 0;
        }
        getAsString() {
            return '';
        }
        toJsonWithId() {
            return {
                type: this.getId(),
                value: this.toJson(),
            };
        }
        static getFactory(id) {
            const factory = this.FACTORIES.get(id);
            if (!factory) {
                throw new Error(`Invalid tag id ${id}`);
            }
            return factory;
        }
        static create(id) {
            return this.getFactory(id).create();
        }
        static fromString(input) {
            const reader = typeof input === 'string' ? new StringReader(input) : input;
            return this.getFactory(NbtType.Compound).fromString(reader);
        }
        static fromJson(value, id = NbtType.Compound) {
            return this.getFactory(id).fromJson(value);
        }
        static fromJsonWithId(value) {
            const obj = Json.readObject(value) ?? {};
            const id = Json.readInt(obj.type) ?? 0;
            return NbtTag.fromJson(obj.value ?? {}, id);
        }
        static fromBytes(input, id = NbtType.Compound) {
            return this.getFactory(id).fromBytes(input);
        }
    }

    class NbtByte extends NbtTag {
        static ZERO = new NbtByte(0);
        static ONE = new NbtByte(1);
        value;
        constructor(value) {
            super();
            this.value = typeof value === 'number' ? value : (value ? 1 : 0);
        }
        getId() {
            return NbtType.Byte;
        }
        equals(other) {
            return other.isByte() && this.value === other.value;
        }
        getAsNumber() {
            return this.value;
        }
        toString() {
            return this.value.toFixed() + 'b';
        }
        toPrettyString() {
            return this.toString();
        }
        toSimplifiedJson() {
            return this.value;
        }
        toJson() {
            return this.value;
        }
        toBytes(output) {
            output.writeByte(this.value);
        }
        static create() {
            return NbtByte.ZERO;
        }
        static fromJson(value) {
            return new NbtByte(Json.readInt(value) ?? 0);
        }
        static fromBytes(input) {
            const value = input.readByte();
            return new NbtByte(value);
        }
    }
    NbtTag.register(NbtType.Byte, NbtByte);

    class NbtAbstractList extends NbtTag {
        items;
        constructor(items) {
            super();
            this.items = items;
        }
        getItems() {
            return this.items.slice(0);
        }
        getAsTuple(length, mapper) {
            return [...Array(length)].map((_, i) => mapper(this.items[i]));
        }
        get(index) {
            index = Math.floor(index);
            if (index < 0 || index >= this.items.length) {
                return undefined;
            }
            return this.items[index];
        }
        get length() {
            return this.items.length;
        }
        map(fn) {
            return this.items.map(fn);
        }
        filter(fn) {
            return this.items.filter(fn);
        }
        forEach(fn) {
            this.items.forEach(fn);
        }
        set(index, tag) {
            this.items[index] = tag;
        }
        add(tag) {
            this.items.push(tag);
        }
        insert(index, tag) {
            this.items.splice(index, 0, tag);
        }
        delete(index) {
            this.items.splice(index, 1);
        }
        clear() {
            this.items = [];
        }
    }

    class NbtByteArray extends NbtAbstractList {
        constructor(items) {
            super(Array.from(items ?? [], e => typeof e === 'number' ? new NbtByte(e) : e));
        }
        getId() {
            return NbtType.ByteArray;
        }
        equals(other) {
            return other.isByteArray()
                && this.length === other.length
                && this.items.every((item, i) => item.equals(other.items[i]));
        }
        getType() {
            return NbtType.Byte;
        }
        toString() {
            const entries = this.items.map(e => e.getAsNumber().toFixed() + 'B');
            return '[B;' + entries.join(',') + ']';
        }
        toPrettyString() {
            return this.toString();
        }
        toSimplifiedJson() {
            return this.items.map(e => e.getAsNumber());
        }
        toJson() {
            return this.items.map(e => e.getAsNumber());
        }
        toBytes(output) {
            output.writeInt(this.items.length);
            output.writeBytes(this.items.map(e => e.getAsNumber()));
        }
        static create() {
            return new NbtByteArray([]);
        }
        static fromJson(value) {
            const items = Json.readArray(value, e => Json.readNumber(e) ?? 0) ?? [];
            return new NbtByteArray(items);
        }
        static fromBytes(input) {
            const length = input.readInt();
            const items = input.readBytes(length);
            return new NbtByteArray(items);
        }
    }
    NbtTag.register(NbtType.ByteArray, NbtByteArray);

    class NbtFloat extends NbtTag {
        value;
        constructor(value) {
            super();
            this.value = value;
        }
        getId() {
            return NbtType.Float;
        }
        equals(other) {
            return other.isFloat() && this.value === other.value;
        }
        getAsNumber() {
            return this.value;
        }
        toString() {
            return this.value.toString() + 'f';
        }
        toPrettyString() {
            return this.toString();
        }
        toSimplifiedJson() {
            return this.value;
        }
        toJson() {
            return this.value;
        }
        toBytes(output) {
            output.writeFloat(this.value);
        }
        static create() {
            return new NbtFloat(0);
        }
        static fromJson(value) {
            return new NbtFloat(Json.readNumber(value) ?? 0);
        }
        static fromBytes(input) {
            const value = input.readFloat();
            return new NbtFloat(value);
        }
    }
    NbtTag.register(NbtType.Float, NbtFloat);

    class NbtInt extends NbtTag {
        value;
        constructor(value) {
            super();
            this.value = value;
        }
        getId() {
            return NbtType.Int;
        }
        equals(other) {
            return other.isInt() && this.value === other.value;
        }
        getAsNumber() {
            return this.value;
        }
        toString() {
            return this.value.toFixed();
        }
        toPrettyString() {
            return this.toString();
        }
        toSimplifiedJson() {
            return this.value;
        }
        toJson() {
            return this.value;
        }
        toBytes(output) {
            output.writeInt(this.value);
        }
        static create() {
            return new NbtInt(0);
        }
        static fromJson(value) {
            return new NbtInt(Json.readInt(value) ?? 0);
        }
        static fromBytes(input) {
            const value = input.readInt();
            return new NbtInt(value);
        }
    }
    NbtTag.register(NbtType.Int, NbtInt);

    class NbtIntArray extends NbtAbstractList {
        constructor(items) {
            super(Array.from(items ?? [], e => typeof e === 'number' ? new NbtInt(e) : e));
        }
        getId() {
            return NbtType.IntArray;
        }
        equals(other) {
            return other.isIntArray()
                && this.length === other.length
                && this.items.every((item, i) => item.equals(other.items[i]));
        }
        getType() {
            return NbtType.Int;
        }
        get length() {
            return this.items.length;
        }
        toString() {
            const entries = this.items.map(e => e.getAsNumber().toFixed());
            return '[I;' + entries.join(',') + ']';
        }
        toPrettyString() {
            return this.toString();
        }
        toSimplifiedJson() {
            return this.items.map(e => e.getAsNumber());
        }
        toJson() {
            return this.items.map(e => e.getAsNumber());
        }
        toBytes(output) {
            output.writeInt(this.items.length);
            for (const entry of this.items) {
                output.writeInt(entry.getAsNumber());
            }
        }
        static create() {
            return new NbtIntArray();
        }
        static fromJson(value) {
            const items = Json.readArray(value, e => Json.readNumber(e) ?? 0) ?? [];
            return new NbtIntArray(items);
        }
        static fromBytes(input) {
            const length = input.readInt();
            const items = [];
            for (let i = 0; i < length; i += 1) {
                items.push(input.readInt());
            }
            return new NbtIntArray(items);
        }
    }
    NbtTag.register(NbtType.IntArray, NbtIntArray);

    class NbtList extends NbtAbstractList {
        type;
        constructor(items, type) {
            super(items ?? []);
            this.type = this.items.length === 0 ? NbtType.End : (type ?? this.items[0].getId());
        }
        static make(factory, items) {
            return new NbtList(items.map(v => new factory(v)));
        }
        getId() {
            return NbtType.List;
        }
        equals(other) {
            return other.isList()
                && this.type === other.type
                && this.length === other.length
                && this.items.every((item, i) => item.equals(other.items[i]));
        }
        getType() {
            return this.type;
        }
        getNumber(index) {
            const entry = this.get(index);
            if (entry?.isNumber()) {
                return entry.getAsNumber();
            }
            return 0;
        }
        getString(index) {
            const entry = this.get(index);
            if (entry?.isString()) {
                return entry.getAsString();
            }
            return '';
        }
        getList(index, type) {
            const entry = this.get(index);
            if (entry?.isList() && entry.getType() === type) {
                return entry;
            }
            return NbtList.create();
        }
        getCompound(index) {
            const entry = this.get(index);
            if (entry?.isCompound()) {
                return entry;
            }
            return NbtCompound.create();
        }
        set(index, tag) {
            this.updateType(tag);
            super.set(index, tag);
        }
        add(tag) {
            this.updateType(tag);
            super.add(tag);
        }
        insert(index, tag) {
            this.updateType(tag);
            super.insert(index, tag);
        }
        updateType(tag) {
            if (tag.getId() === NbtType.End) {
                return;
            }
            else if (this.type === NbtType.End) {
                this.type = tag.getId();
            }
            else if (this.type !== tag.getId()) {
                throw new Error(`Trying to add tag of type ${NbtType[tag.getId()]} to list of ${NbtType[this.type]}`);
            }
        }
        clear() {
            super.clear();
            this.type = NbtType.End;
        }
        toString() {
            return '[' + this.items.map(i => i.toString()).join(',') + ']';
        }
        toPrettyString(indent = '  ', depth = 0) {
            if (this.length === 0)
                return '[]';
            const i = indent.repeat(depth);
            const ii = indent.repeat(depth + 1);
            return '[\n' + this.map(value => {
                return ii + value.toPrettyString(indent, depth + 1);
            }).join(',\n') + '\n' + i + ']';
        }
        toSimplifiedJson() {
            return this.map(e => e.toSimplifiedJson());
        }
        toJson() {
            return {
                type: this.type,
                items: this.items.map(e => e.toJson()),
            };
        }
        toBytes(output) {
            if (this.items.length === 0) {
                this.type = NbtType.End;
            }
            else {
                this.type = this.items[0].getId();
            }
            output.writeByte(this.type);
            output.writeInt(this.items.length);
            for (const tag of this.items) {
                tag.toBytes(output);
            }
        }
        static create() {
            return new NbtList();
        }
        static fromJson(value) {
            const obj = Json.readObject(value) ?? {};
            const type = Json.readNumber(obj.type) ?? NbtType.Compound;
            const items = (Json.readArray(obj.items) ?? [])
                .flatMap(v => v !== undefined ? [NbtTag.fromJson(v, type)] : []);
            return new NbtList(items, type);
        }
        static fromBytes(input) {
            const type = input.readByte();
            const length = input.readInt();
            if (type === NbtType.End && length > 0) {
                throw new Error(`Missing type on ListTag but length is ${length}`);
            }
            const items = [];
            for (let i = 0; i < length; i += 1) {
                items.push(NbtTag.fromBytes(input, type));
            }
            return new NbtList(items, type);
        }
    }
    NbtTag.register(NbtType.List, NbtList);

    class NbtLong extends NbtTag {
        static dataview = new DataView(new Uint8Array(8).buffer);
        value;
        constructor(value) {
            super();
            this.value = NbtLong.toPair(value);
        }
        static toPair(value) {
            return Array.isArray(value) ? value : NbtLong.bigintToPair(value);
        }
        static bigintToPair(value) {
            NbtLong.dataview.setBigInt64(0, value);
            return [NbtLong.dataview.getInt32(0), NbtLong.dataview.getInt32(4)];
        }
        static pairToBigint(value) {
            NbtLong.dataview.setInt32(0, Number(value[0]));
            NbtLong.dataview.setInt32(4, Number(value[1]));
            return NbtLong.dataview.getBigInt64(0);
        }
        static pairToString(value) {
            return NbtLong.pairToBigint(value).toString();
        }
        static pairToNumber(value) {
            return Number(NbtLong.pairToBigint(value));
        }
        getId() {
            return NbtType.Long;
        }
        equals(other) {
            return other.isLong()
                && this.value[0] === other.value[0]
                && this.value[1] === other.value[1];
        }
        getAsNumber() {
            return NbtLong.pairToNumber(this.value);
        }
        getAsPair() {
            return this.value;
        }
        toBigInt() {
            return NbtLong.pairToBigint(this.value);
        }
        toString() {
            return NbtLong.pairToString(this.value) + 'L';
        }
        toPrettyString() {
            return this.toString();
        }
        toSimplifiedJson() {
            return NbtLong.pairToNumber(this.value);
        }
        toJson() {
            return this.value;
        }
        toBytes(output) {
            output.writeInt(this.value[0]);
            output.writeInt(this.value[1]);
        }
        static create() {
            return new NbtLong([0, 0]);
        }
        static fromJson(value) {
            return new NbtLong(Array.isArray(value) && value.length === 2
                ? value.map(e => typeof e === 'number' ? e : 0)
                : [0, 0]);
        }
        static fromBytes(input) {
            const lo = input.readInt();
            const hi = input.readInt();
            return new NbtLong([lo, hi]);
        }
    }
    NbtTag.register(NbtType.Long, NbtLong);

    class NbtLongArray extends NbtAbstractList {
        constructor(items) {
            super(Array.from(items ?? [], e => typeof e === 'bigint' || Array.isArray(e) ? new NbtLong(e) : e));
        }
        getId() {
            return NbtType.LongArray;
        }
        equals(other) {
            return other.isLongArray()
                && this.length === other.length
                && this.items.every((item, i) => item.equals(other.items[i]));
        }
        getType() {
            return NbtType.Long;
        }
        get length() {
            return this.items.length;
        }
        toString() {
            const entries = this.items.map(e => e.toString());
            return '[I;' + entries.join(',') + ']';
        }
        toPrettyString() {
            return this.toString();
        }
        toSimplifiedJson() {
            return this.items.map(e => e.getAsPair());
        }
        toJson() {
            return this.items.map(e => e.getAsPair());
        }
        toBytes(output) {
            output.writeInt(this.items.length);
            for (const entry of this.items) {
                const [hi, lo] = entry.getAsPair();
                output.writeInt(hi);
                output.writeInt(lo);
            }
        }
        static create() {
            return new NbtLongArray();
        }
        static fromJson(value) {
            const items = Json.readArray(value, e => Json.readPair(e, f => Json.readNumber(f) ?? 0) ?? [0, 0]) ?? [];
            return new NbtLongArray(items);
        }
        static fromBytes(input) {
            const length = input.readInt();
            const items = [];
            for (let i = 0; i < length; i += 1) {
                items.push([input.readInt(), input.readInt()]);
            }
            return new NbtLongArray(items);
        }
    }
    NbtTag.register(NbtType.LongArray, NbtLongArray);

    class NbtShort extends NbtTag {
        value;
        constructor(value) {
            super();
            this.value = value;
        }
        getId() {
            return NbtType.Short;
        }
        equals(other) {
            return other.isShort() && this.value === other.value;
        }
        getAsNumber() {
            return this.value;
        }
        toString() {
            return this.value.toFixed() + 's';
        }
        toPrettyString() {
            return this.toString();
        }
        toSimplifiedJson() {
            return this.value;
        }
        toJson() {
            return this.value;
        }
        toBytes(output) {
            output.writeShort(this.value);
        }
        static create() {
            return new NbtShort(0);
        }
        static fromJson(value) {
            return new NbtShort(typeof value === 'number' ? Math.floor(value) : 0);
        }
        static fromBytes(input) {
            const value = input.readShort();
            return new NbtShort(value);
        }
    }
    NbtTag.register(NbtType.Short, NbtShort);

    class NbtString extends NbtTag {
        static EMPTY = new NbtString('');
        value;
        constructor(value) {
            super();
            this.value = value;
        }
        getId() {
            return NbtType.String;
        }
        equals(other) {
            return other.isString() && this.value === other.value;
        }
        getAsString() {
            return this.value;
        }
        toString() {
            return '"' + this.value.replace(/(\\|")/g, '\\$1') + '"';
        }
        toPrettyString() {
            return this.toString();
        }
        toSimplifiedJson() {
            return this.value;
        }
        toJson() {
            return this.value;
        }
        toBytes(output) {
            output.writeString(this.value);
        }
        static create() {
            return NbtString.EMPTY;
        }
        static fromJson(value) {
            return new NbtString(typeof value === 'string' ? value : '');
        }
        static fromBytes(input) {
            const value = input.readString();
            return new NbtString(value);
        }
    }
    NbtTag.register(NbtType.String, NbtString);

    /**
     * SNBT Parser
     */
    var NbtParser;
    (function (NbtParser) {
        const DOUBLE_PATTERN_NOSUFFIX = new RegExp('^[-+]?(?:[0-9]+[.]|[0-9]*[.][0-9]+)(?:e[-+]?[0-9]+)?$', 'i');
        const DOUBLE_PATTERN = new RegExp('^[-+]?(?:[0-9]+[.]?|[0-9]*[.][0-9]+)(?:e[-+]?[0-9]+)?d$', 'i');
        const FLOAT_PATTERN = new RegExp('^[-+]?(?:[0-9]+[.]?|[0-9]*[.][0-9]+)(?:e[-+]?[0-9]+)?f$', 'i');
        const BYTE_PATTERN = new RegExp('^[-+]?(?:0|[1-9][0-9]*)b$', 'i');
        const LONG_PATTERN = new RegExp('^[-+]?(?:0|[1-9][0-9]*)l$', 'i');
        const SHORT_PATTERN = new RegExp('^[-+]?(?:0|[1-9][0-9]*)s$', 'i');
        const INT_PATTERN = new RegExp('^[-+]?(?:0|[1-9][0-9]*)$', 'i');
        function readTag(reader) {
            reader.skipWhitespace();
            if (!reader.canRead()) {
                throw reader.createError('Expected value');
            }
            const c = reader.peek();
            if (c === '{') {
                return readCompound(reader);
            }
            else if (c === '[') {
                if (reader.canRead(3) && !StringReader.isQuotedStringStart(reader.peek(1)) && reader.peek(2) === ';') {
                    reader.expect('[', true);
                    const start = reader.cursor;
                    const d = reader.read();
                    reader.skip();
                    reader.skipWhitespace();
                    if (!reader.canRead()) {
                        throw reader.createError('Expected value');
                    }
                    else if (d === 'B') {
                        return readArray(reader, NbtByteArray, NbtType.ByteArray, NbtType.Byte);
                    }
                    else if (d === 'L') {
                        return readArray(reader, NbtLongArray, NbtType.LongArray, NbtType.Long);
                    }
                    else if (d === 'I') {
                        return readArray(reader, NbtIntArray, NbtType.IntArray, NbtType.Int);
                    }
                    else {
                        reader.cursor = start;
                        throw reader.createError(`Invalid array type '${d}'`);
                    }
                }
                else {
                    return readList(reader);
                }
            }
            else {
                reader.skipWhitespace();
                const start = reader.cursor;
                if (StringReader.isQuotedStringStart(reader.peek())) {
                    return new NbtString(reader.readQuotedString());
                }
                else {
                    const value = reader.readUnquotedString();
                    if (value.length === 0) {
                        reader.cursor = start;
                        throw reader.createError('Expected value');
                    }
                    try {
                        if (FLOAT_PATTERN.test(value)) {
                            const number = Number(value.substring(0, value.length - 1));
                            return new NbtFloat(number);
                        }
                        else if (BYTE_PATTERN.test(value)) {
                            const number = Number(value.substring(0, value.length - 1));
                            return new NbtByte(Math.floor(number));
                        }
                        else if (LONG_PATTERN.test(value)) {
                            const number = BigInt(value.substring(0, value.length - 1));
                            return new NbtLong(number);
                        }
                        else if (SHORT_PATTERN.test(value)) {
                            const number = Number(value.substring(0, value.length - 1));
                            return new NbtShort(Math.floor(number));
                        }
                        else if (INT_PATTERN.test(value)) {
                            const number = Number(value);
                            return new NbtInt(Math.floor(number));
                        }
                        else if (DOUBLE_PATTERN.test(value)) {
                            const number = Number(value.substring(0, value.length - 1));
                            return new NbtDouble(number);
                        }
                        else if (DOUBLE_PATTERN_NOSUFFIX.test(value)) {
                            const number = Number(value);
                            return new NbtDouble(number);
                        }
                        else if (value.toLowerCase() === 'true') {
                            return NbtByte.ONE;
                        }
                        else if (value.toLowerCase() === 'false') {
                            return NbtByte.ZERO;
                        }
                    }
                    catch (e) { }
                    return value.length === 0 ? NbtString.EMPTY : new NbtString(value);
                }
            }
        }
        NbtParser.readTag = readTag;
        function readCompound(reader) {
            reader.expect('{', true);
            const properties = new Map();
            reader.skipWhitespace();
            while (reader.canRead() && reader.peek() !== '}') {
                const start = reader.cursor;
                reader.skipWhitespace();
                if (!reader.canRead()) {
                    throw reader.createError('Expected key');
                }
                const key = reader.readString();
                if (key.length === 0) {
                    reader.cursor = start;
                    throw reader.createError('Expected key');
                }
                reader.expect(':', true);
                const value = readTag(reader);
                properties.set(key, value);
                if (!hasElementSeparator(reader)) {
                    break;
                }
                if (!reader.canRead()) {
                    throw reader.createError('Expected key');
                }
            }
            reader.expect('}', true);
            return new NbtCompound(properties);
        }
        function readList(reader) {
            reader.expect('[', true);
            reader.skipWhitespace();
            if (!reader.canRead()) {
                throw reader.createError('Expected value');
            }
            const items = [];
            let type = NbtType.End;
            while (reader.peek() !== ']') {
                const start = reader.cursor;
                const value = readTag(reader);
                const valueId = value.getId();
                if (type === NbtType.End) {
                    type = valueId;
                }
                else if (valueId !== type) {
                    reader.cursor = start;
                    throw reader.createError(`Can't insert ${NbtType[valueId]} into list of ${NbtType[type]}`);
                }
                items.push(value);
                if (!hasElementSeparator(reader)) {
                    break;
                }
                if (!reader.canRead()) {
                    throw reader.createError('Expected value');
                }
            }
            reader.expect(']', true);
            return new NbtList(items, type);
        }
        function readArray(reader, factory, arrayId, childId) {
            const data = [];
            while (reader.peek() !== ']') {
                const entry = readTag(reader);
                if (entry.getId() !== childId) {
                    throw reader.createError(`Can't insert ${NbtType[entry.getId()]} into ${NbtType[arrayId]}`);
                }
                data.push((entry.isLong() ? entry.getAsPair() : entry.getAsNumber()));
                if (!hasElementSeparator(reader)) {
                    break;
                }
                if (!reader.canRead()) {
                    throw reader.createError('Expected value');
                }
            }
            reader.expect(']');
            return new factory(data);
        }
        function hasElementSeparator(reader) {
            reader.skipWhitespace();
            if (reader.canRead() && reader.peek() === ',') {
                reader.skip();
                reader.skipWhitespace();
                return true;
            }
            else {
                return false;
            }
        }
    })(NbtParser || (NbtParser = {}));

    class NbtCompound extends NbtTag {
        properties;
        constructor(properties) {
            super();
            this.properties = properties ?? new Map();
        }
        getId() {
            return NbtType.Compound;
        }
        equals(other) {
            return other.isCompound()
                && this.size === other.size
                && [...this.properties.entries()].every(([key, value]) => {
                    const otherValue = other.properties.get(key);
                    return otherValue !== undefined && value.equals(otherValue);
                });
        }
        has(key) {
            return this.properties.has(key);
        }
        hasNumber(key) {
            return this.get(key)?.isNumber() ?? false;
        }
        hasString(key) {
            return this.get(key)?.isString() ?? false;
        }
        hasList(key, type, length) {
            const tag = this.get(key);
            return (tag?.isList()
                && (type === undefined || tag.getType() === type)
                && (length === undefined || tag.length === length)) ?? false;
        }
        hasCompound(key) {
            return this.get(key)?.isCompound() ?? false;
        }
        get(key) {
            return this.properties.get(key);
        }
        getString(key) {
            return this.get(key)?.getAsString() ?? '';
        }
        getNumber(key) {
            return this.get(key)?.getAsNumber() ?? 0;
        }
        getBoolean(key) {
            return this.getNumber(key) !== 0;
        }
        getList(key, type) {
            const tag = this.get(key);
            if (tag?.isList() && (type === undefined || tag.getType() === type)) {
                return tag;
            }
            return NbtList.create();
        }
        getCompound(key) {
            const tag = this.get(key);
            if (tag?.isCompound()) {
                return tag;
            }
            return NbtCompound.create();
        }
        getByteArray(key) {
            const tag = this.get(key);
            if (tag?.isByteArray()) {
                return tag;
            }
            return NbtByteArray.create();
        }
        getIntArray(key) {
            const tag = this.get(key);
            if (tag?.isIntArray()) {
                return tag;
            }
            return NbtIntArray.create();
        }
        getLongArray(key) {
            const tag = this.get(key);
            if (tag?.isLongArray()) {
                return tag;
            }
            return NbtLongArray.create();
        }
        keys() {
            return this.properties.keys();
        }
        get size() {
            return this.properties.size;
        }
        map(fn) {
            return Object.fromEntries([...this.properties.entries()]
                .map(([key, value]) => fn(key, value, this)));
        }
        forEach(fn) {
            [...this.properties.entries()]
                .forEach(([key, value]) => fn(key, value, this));
        }
        set(key, value) {
            this.properties.set(key, value);
            return this;
        }
        delete(key) {
            return this.properties.delete(key);
        }
        clear() {
            this.properties.clear();
            return this;
        }
        toString() {
            const pairs = [];
            for (const [key, tag] of this.properties.entries()) {
                const needsQuotes = key.split('').some(c => !StringReader.isAllowedInUnquotedString(c));
                pairs.push((needsQuotes ? JSON.stringify(key) : key) + ':' + tag.toString());
            }
            return '{' + pairs.join(',') + '}';
        }
        toPrettyString(indent = '  ', depth = 0) {
            if (this.size === 0)
                return '{}';
            const i = indent.repeat(depth);
            const ii = indent.repeat(depth + 1);
            const pairs = [];
            for (const [key, tag] of this.properties.entries()) {
                const needsQuotes = key.split('').some(c => !StringReader.isAllowedInUnquotedString(c));
                pairs.push((needsQuotes ? JSON.stringify(key) : key) + ': ' + tag.toPrettyString(indent, depth + 1));
            }
            return '{\n' + pairs.map(p => ii + p).join(',\n') + '\n' + i + '}';
        }
        toSimplifiedJson() {
            return this.map((key, value) => [key, value.toSimplifiedJson()]);
        }
        toJson() {
            return this.map((key, value) => [key, {
                    type: value.getId(),
                    value: value.toJson(),
                }]);
        }
        toBytes(output) {
            for (const [key, tag] of this.properties.entries()) {
                const id = tag.getId();
                output.writeByte(id);
                output.writeString(key);
                tag.toBytes(output);
            }
            output.writeByte(NbtType.End);
        }
        static create() {
            return new NbtCompound();
        }
        static fromString(reader) {
            return NbtParser.readTag(reader);
        }
        static fromJson(value) {
            const properties = Json.readMap(value, e => {
                const { type, value } = Json.readObject(e) ?? {};
                const id = Json.readNumber(type);
                const tag = NbtTag.fromJson(value ?? {}, id);
                return tag;
            });
            return new NbtCompound(new Map(Object.entries(properties)));
        }
        static fromBytes(input) {
            const properties = new Map();
            while (true) {
                const id = input.readByte();
                if (id === NbtType.End)
                    break;
                const key = input.readString();
                const value = NbtTag.fromBytes(input, id);
                properties.set(key, value);
            }
            return new NbtCompound(properties);
        }
    }
    NbtTag.register(NbtType.Compound, NbtCompound);

    class NbtFile {
        name;
        root;
        compression;
        littleEndian;
        bedrockHeader;
        static DEFAULT_NAME = '';
        static DEFAULT_BEDROCK_HEADER = 4;
        constructor(name, root, compression, littleEndian, bedrockHeader) {
            this.name = name;
            this.root = root;
            this.compression = compression;
            this.littleEndian = littleEndian;
            this.bedrockHeader = bedrockHeader;
        }
        writeNamedTag(output) {
            output.writeByte(NbtType.Compound);
            output.writeString(this.name);
            this.root.toBytes(output);
        }
        write() {
            const littleEndian = this.littleEndian === true || this.bedrockHeader !== undefined;
            const output = new RawDataOutput({ littleEndian, offset: this.bedrockHeader && 8 });
            this.writeNamedTag(output);
            if (this.bedrockHeader !== undefined) {
                const end = output.offset;
                output.offset = 0;
                output.writeInt(this.bedrockHeader);
                output.writeInt(end - 8);
                output.offset = end;
            }
            const array = output.getData();
            if (this.compression === 'gzip') {
                return pako.gzip(array);
            }
            else if (this.compression === 'zlib') {
                return pako.deflate(array);
            }
            return array;
        }
        static readNamedTag(input) {
            if (input.readByte() !== NbtType.Compound) {
                throw new Error('Top tag should be a compound');
            }
            return {
                name: input.readString(),
                root: NbtCompound.fromBytes(input),
            };
        }
        static create(options = {}) {
            const name = options.name ?? NbtFile.DEFAULT_NAME;
            const root = NbtCompound.create();
            const compression = options.compression ?? 'none';
            const bedrockHeader = typeof options.bedrockHeader === 'boolean' ? NbtFile.DEFAULT_BEDROCK_HEADER : options.bedrockHeader;
            const littleEndian = options.littleEndian ?? options.bedrockHeader !== undefined;
            return new NbtFile(name, root, compression, littleEndian, bedrockHeader);
        }
        static read(array, options = {}) {
            const bedrockHeader = typeof options.bedrockHeader === 'number' ? options.bedrockHeader : (options.bedrockHeader ? getBedrockHeader(array) : undefined);
            const isGzipCompressed = options.compression === 'gzip' ||
                (!bedrockHeader && options.compression === undefined && hasGzipHeader(array));
            const isZlibCompressed = options.compression === 'zlib' ||
                (!bedrockHeader && options.compression === undefined && hasZlibHeader(array));
            const uncompressedData = (isZlibCompressed || isGzipCompressed) ? pako.inflate(array) : array;
            const littleEndian = options.littleEndian || bedrockHeader !== undefined;
            const compression = isGzipCompressed ? 'gzip' : isZlibCompressed ? 'zlib' : 'none';
            const input = new RawDataInput(uncompressedData, { littleEndian, offset: bedrockHeader && 8 });
            const { name, root } = NbtFile.readNamedTag(input);
            return new NbtFile(options.name ?? name, root, compression, littleEndian, bedrockHeader);
        }
        toJson() {
            return {
                name: this.name,
                root: this.root.toJson(),
                compression: this.compression,
                littleEndian: this.littleEndian,
                bedrockHeader: this.bedrockHeader ?? null,
            };
        }
        static fromJson(value) {
            const obj = Json.readObject(value) ?? {};
            const name = Json.readString(obj.name) ?? '';
            const root = NbtCompound.fromJson(obj.root ?? {});
            const compression = (Json.readString(obj.compression) ?? 'none');
            const littleEndian = Json.readBoolean(obj.littleEndian) ?? false;
            const bedrockHeader = Json.readNumber(obj.bedrockHeader);
            return new NbtFile(name, root, compression, littleEndian, bedrockHeader);
        }
    }

    class NbtChunk {
        x;
        z;
        compression;
        timestamp;
        raw;
        file;
        dirty;
        constructor(x, z, compression, timestamp, raw) {
            this.x = x;
            this.z = z;
            this.compression = compression;
            this.timestamp = timestamp;
            this.raw = raw;
            this.dirty = false;
        }
        getCompression() {
            switch (this.compression) {
                case 1: return 'gzip';
                case 2: return 'zlib';
                case 3: return 'none';
                default: throw new Error(`Invalid compression mode ${this.compression}`);
            }
        }
        setCompression(compression) {
            switch (compression) {
                case 'gzip':
                    this.compression = 1;
                    break;
                case 'zlib':
                    this.compression = 2;
                    break;
                case 'none':
                    this.compression = 3;
                    break;
                default: throw new Error(`Invalid compression mode ${compression}`);
            }
        }
        getFile() {
            if (this.file === undefined) {
                this.file = NbtFile.read(this.raw, {
                    compression: this.getCompression(),
                });
            }
            return this.file;
        }
        getRoot() {
            return this.getFile().root;
        }
        setRoot(root) {
            if (this.file === undefined) {
                this.file = NbtFile.create({
                    compression: this.getCompression(),
                });
            }
            this.file.root = root;
            this.markDirty();
        }
        markDirty() {
            this.dirty = true;
        }
        getRaw() {
            if (this.file === undefined || this.dirty === false) {
                return this.raw;
            }
            this.file.compression = this.getCompression();
            const array = this.file.write();
            this.raw = array;
            this.dirty = false;
            return array;
        }
        toJson() {
            return {
                x: this.x,
                z: this.z,
                compression: this.compression,
                timestamp: this.timestamp,
                size: this.raw.byteLength,
            };
        }
        toRef(resolver) {
            return new NbtChunk.Ref(this.x, this.z, this.compression, this.timestamp, this.raw.byteLength, resolver);
        }
        static create(x, z, file, timestamp) {
            const chunk = new NbtChunk(x, z, 0, timestamp ?? 0, file.write());
            chunk.setCompression(file.compression);
            return chunk;
        }
        static fromJson(value, resolver) {
            const obj = Json.readObject(value) ?? {};
            const x = Json.readInt(obj.x) ?? 0;
            const z = Json.readInt(obj.z) ?? 0;
            const compression = Json.readNumber(obj.compression) ?? 2;
            const timestamp = Json.readInt(obj.timestamp) ?? 0;
            const size = Json.readInt(obj.size) ?? 0;
            return new NbtChunk.Ref(x, z, compression, timestamp, size, resolver);
        }
    }
    (function (NbtChunk) {
        class Ref {
            x;
            z;
            compression;
            timestamp;
            size;
            resolver;
            file;
            constructor(x, z, compression, timestamp, size, resolver) {
                this.x = x;
                this.z = z;
                this.compression = compression;
                this.timestamp = timestamp;
                this.size = size;
                this.resolver = resolver;
            }
            getFile() {
                if (this.file instanceof NbtFile) {
                    return this.file;
                }
                return undefined;
            }
            getRoot() {
                if (this.file instanceof NbtFile) {
                    return this.file.root;
                }
                return undefined;
            }
            async getFileAsync() {
                if (this.file) {
                    return this.file;
                }
                this.file = (async () => {
                    const file = await this.resolver(this.x, this.z);
                    this.file = file;
                    return file;
                })();
                return this.file;
            }
            async getRootAsync() {
                const file = await this.getFileAsync();
                return file.root;
            }
            isResolved() {
                return this.file instanceof NbtFile;
            }
        }
        NbtChunk.Ref = Ref;
    })(NbtChunk || (NbtChunk = {}));

    class NbtAbstractRegion {
        chunks;
        constructor(chunks) {
            this.chunks = Array(32 * 32).fill(undefined);
            for (const chunk of chunks) {
                const index = NbtRegion.getIndex(chunk.x, chunk.z);
                this.chunks[index] = chunk;
            }
        }
        getChunkPositions() {
            return this.chunks.flatMap(c => c ? [[c.x, c.z]] : []);
        }
        getChunk(index) {
            if (index < 0 || index >= 32 * 32) {
                return undefined;
            }
            return this.chunks[index];
        }
        findChunk(x, z) {
            return this.getChunk(NbtRegion.getIndex(x, z));
        }
        getFirstChunk() {
            return this.chunks.filter(c => c !== undefined)[0];
        }
        filter(predicate) {
            return this.chunks.filter((c) => c !== undefined && predicate(c));
        }
        map(mapper) {
            return this.chunks.flatMap(c => c !== undefined ? [mapper(c)] : []);
        }
    }
    class NbtRegion extends NbtAbstractRegion {
        constructor(chunks) {
            super(chunks);
        }
        write() {
            let totalSectors = 0;
            for (const chunk of this.chunks) {
                if (chunk === undefined)
                    continue;
                totalSectors += Math.ceil(chunk.getRaw().length / 4096);
            }
            const array = new Uint8Array(8192 + totalSectors * 4096);
            const dataView = new DataView(array.buffer);
            let offset = 2;
            for (const chunk of this.chunks) {
                if (chunk === undefined)
                    continue;
                const chunkData = chunk.getRaw();
                const i = 4 * ((chunk.x & 31) + (chunk.z & 31) * 32);
                const sectors = Math.ceil(chunkData.length / 4096);
                dataView.setInt8(i, offset >> 16);
                dataView.setInt16(i + 1, offset & 0xffff);
                dataView.setInt8(i + 3, sectors);
                dataView.setInt32(i + 4096, chunk.timestamp);
                const j = offset * 4096;
                dataView.setInt32(j, chunkData.length + 1);
                dataView.setInt8(j + 4, chunk.compression);
                array.set(chunkData, j + 5);
                offset += sectors;
            }
            return array;
        }
        static read(array) {
            const chunks = [];
            for (let x = 0; x < 32; x += 1) {
                for (let z = 0; z < 32; z += 1) {
                    const i = 4 * ((x & 31) + (z & 31) * 32);
                    const sectors = array[i + 3];
                    if (sectors === 0)
                        continue;
                    const offset = (array[i] << 16) + (array[i + 1] << 8) + array[i + 2];
                    const timestamp = (array[i + 4096] << 24) + (array[i + 4097] << 16) + (array[i + 4098] << 8) + array[i + 4099];
                    const j = offset * 4096;
                    const length = (array[j] << 24) + (array[j + 1] << 16) + (array[j + 2] << 8) + array[j + 3];
                    const compression = array[j + 4];
                    const data = array.slice(j + 5, j + 4 + length);
                    chunks.push(new NbtChunk(x, z, compression, timestamp, data));
                }
            }
            return new NbtRegion(chunks);
        }
        static getIndex(x, z) {
            return (x & 31) + (z & 31) * 32;
        }
        toJson() {
            return {
                chunks: this.map(c => c.toJson()),
            };
        }
        static fromJson(value, chunkResolver) {
            const obj = Json.readObject(value) ?? {};
            const chunks = Json.readArray(obj.chunks) ?? [];
            const chunks2 = chunks.flatMap(c => c !== undefined ? [NbtChunk.fromJson(c, chunkResolver)] : []);
            return new NbtRegion.Ref(chunks2);
        }
    }
    (function (NbtRegion) {
        class Ref extends NbtAbstractRegion {
        }
        NbtRegion.Ref = Ref;
    })(NbtRegion || (NbtRegion = {}));

    class NbtDouble extends NbtTag {
        value;
        constructor(value) {
            super();
            this.value = value;
        }
        getId() {
            return NbtType.Double;
        }
        equals(other) {
            return other.isDouble() && this.value === other.value;
        }
        getAsNumber() {
            return this.value;
        }
        toString() {
            if (Number.isInteger(this.value)) {
                return this.value.toFixed(1);
            }
            return this.value.toString();
        }
        toPrettyString() {
            return this.toString();
        }
        toSimplifiedJson() {
            return this.value;
        }
        toJson() {
            return this.value;
        }
        toBytes(output) {
            output.writeDouble(this.value);
        }
        static create() {
            return new NbtDouble(0);
        }
        static fromJson(value) {
            return new NbtDouble(Json.readNumber(value) ?? 0);
        }
        static fromBytes(input) {
            const value = input.readDouble();
            return new NbtDouble(value);
        }
    }
    NbtTag.register(NbtType.Double, NbtDouble);

    class NbtEnd extends NbtTag {
        static INSTANCE = new NbtEnd();
        constructor() {
            super();
        }
        getId() {
            return NbtType.End;
        }
        equals(other) {
            return other.isEnd();
        }
        toString() {
            return 'END';
        }
        toPrettyString() {
            return this.toString();
        }
        toSimplifiedJson() {
            return null;
        }
        toJson() {
            return null;
        }
        toBytes() {
        }
        static create() {
            return NbtEnd.INSTANCE;
        }
        static fromJson() {
            return NbtEnd.INSTANCE;
        }
        static fromBytes() {
            return NbtEnd.INSTANCE;
        }
    }
    NbtTag.register(NbtType.End, NbtEnd);

    var Direction;
    (function (Direction) {
        Direction["UP"] = "up";
        Direction["DOWN"] = "down";
        Direction["NORTH"] = "north";
        Direction["EAST"] = "east";
        Direction["SOUTH"] = "south";
        Direction["WEST"] = "west";
    })(Direction || (Direction = {}));
    const directionNormals = {
        [Direction.UP]: [0, 1, 0],
        [Direction.DOWN]: [0, -1, 0],
        [Direction.NORTH]: [0, 0, -1],
        [Direction.EAST]: [1, 0, 0],
        [Direction.SOUTH]: [0, 0, 1],
        [Direction.WEST]: [-1, 0, 0],
    };
    (function (Direction) {
        Direction.ALL = [Direction.UP, Direction.DOWN, Direction.NORTH, Direction.EAST, Direction.SOUTH, Direction.WEST];
        function normal(dir) {
            return directionNormals[dir];
        }
        Direction.normal = normal;
    })(Direction || (Direction = {}));

    var BlockPos;
    (function (BlockPos) {
        function create(x, y, z) {
            return [x, y, z];
        }
        BlockPos.create = create;
        BlockPos.ZERO = BlockPos.create(0, 0, 0);
        function offset(pos, dx, dy, dz) {
            return [pos[0] + dx, pos[1] + dy, pos[2] + dz];
        }
        BlockPos.offset = offset;
        function subtract(pos, other) {
            return [pos[0] - other[0], pos[1] - other[1], pos[2] - other[2]];
        }
        BlockPos.subtract = subtract;
        function add(pos, other) {
            return [pos[0] + other[0], pos[1] + other[1], pos[2] + other[2]];
        }
        BlockPos.add = add;
        function towards(pos, dir) {
            return BlockPos.offset(pos, ...Direction.normal(dir));
        }
        BlockPos.towards = towards;
        function equals(a, b) {
            if (a === b)
                return true;
            return a[0] === b[0] && a[1] === b[1] && a[2] === b[2];
        }
        BlockPos.equals = equals;
        function magnitude(pos) {
            return pos[0] * pos[0] + pos[1] * pos[1] + pos[2] * pos[2];
        }
        BlockPos.magnitude = magnitude;
        function toNbt(pos) {
            return new NbtList(pos.map(e => new NbtInt(e)));
        }
        BlockPos.toNbt = toNbt;
        function fromNbt(nbt) {
            return nbt.getAsTuple(3, e => e?.isInt() ? e.getAsNumber() : 0);
        }
        BlockPos.fromNbt = fromNbt;
        function fromJson(obj) {
            const array = Json.readArray(obj, (e) => Json.readInt(e) ?? 0) ?? [0, 0, 0];
            return create(array[0], array[1], array[2]);
        }
        BlockPos.fromJson = fromJson;
    })(BlockPos || (BlockPos = {}));

    class Identifier {
        namespace;
        path;
        static DEFAULT_NAMESPACE = 'minecraft';
        static SEPARATOR = ':';
        constructor(namespace, path) {
            this.namespace = namespace;
            this.path = path;
        }
        is(other) {
            return this.equals(Identifier.parse(other));
        }
        equals(other) {
            if (this === other) {
                return true;
            }
            if (!(other instanceof Identifier)) {
                return false;
            }
            return this.namespace === other.namespace && this.path === other.path;
        }
        toString() {
            return this.namespace + Identifier.SEPARATOR + this.path;
        }
        withPrefix(prefix) {
            return new Identifier(this.namespace, prefix + this.path);
        }
        static create(path) {
            return new Identifier(this.DEFAULT_NAMESPACE, path);
        }
        static parse(id) {
            const sep = id.indexOf(this.SEPARATOR);
            if (sep >= 0) {
                const namespace = sep >= 1 ? id.substring(0, sep) : this.DEFAULT_NAMESPACE;
                const path = id.substring(sep + 1);
                return new Identifier(namespace, path);
            }
            return new Identifier(this.DEFAULT_NAMESPACE, id);
        }
    }

    class BlockState {
        properties;
        static AIR = new BlockState(Identifier.create('air'));
        static STONE = new BlockState(Identifier.create('stone'));
        static WATER = new BlockState(Identifier.create('water'), { level: '0' });
        static LAVA = new BlockState(Identifier.create('lava'), { level: '0' });
        name;
        constructor(name, properties = {}) {
            this.properties = properties;
            this.name = typeof name === 'string' ? Identifier.parse(name) : name;
        }
        getName() {
            return this.name;
        }
        getProperties() {
            return this.properties;
        }
        getProperty(key) {
            return this.properties[key];
        }
        isFluid() {
            return this.is(BlockState.WATER) || this.is(BlockState.LAVA);
        }
        isWaterlogged() {
            return this.is(BlockState.WATER) || this.is(BlockState.LAVA)
                || this.is('bubble_column')
                || this.is('kelp') || this.is('kelp_plant')
                || this.is('seagrass') || this.is('tall_seagrass')
                || this.properties['waterlogged'] === 'true';
        }
        equals(other) {
            if (!this.name.equals(other.name)) {
                return false;
            }
            if (Object.keys(this.properties).length !== Object.keys(other.properties).length) {
                return false;
            }
            return Object.keys(this.properties).every(p => {
                return other.properties[p] === this.properties[p];
            });
        }
        is(other) {
            if (typeof other === 'string') {
                return this.name.equals(Identifier.parse(other));
            }
            if (other instanceof Identifier) {
                return this.name.equals(other);
            }
            return this.name.equals(other.name);
        }
        toString() {
            if (Object.keys(this.properties).length === 0) {
                return this.name.toString();
            }
            return `${this.name.toString()}[${Object.entries(this.properties).map(([k, v]) => k + '=' + v).join(',')}]`;
        }
        static parse(str) {
            const stateStart = str.indexOf('[');
            if (stateStart === -1) {
                return new BlockState(str);
            }
            else {
                const blockId = str.substring(0, stateStart);
                const states = str.substring(stateStart + 1, str.length - 1).split(',');
                const properties = Object.fromEntries(states.map(e => e.split('=')));
                return new BlockState(blockId, properties);
            }
        }
        static fromNbt(nbt) {
            const name = Identifier.parse(nbt.getString('Name'));
            const properties = nbt.getCompound('Properties')
                .map((key, value) => [key, value.getAsString()]);
            return new BlockState(name, properties);
        }
        static fromJson(obj) {
            const root = Json.readObject(obj) ?? {};
            const name = Identifier.parse(Json.readString(root.Name) ?? BlockState.STONE.name.toString());
            const properties = Json.readMap(root.Properties, p => Json.readString(p) ?? '');
            return new BlockState(name, properties);
        }
    }

    class PalettedContainer {
        size;
        defaultValue;
        storage;
        palette;
        constructor(size, defaultValue) {
            this.size = size;
            this.defaultValue = defaultValue;
            this.storage = Array(size).fill(0);
            this.palette = [defaultValue];
        }
        index(x, y, z) {
            return (x << 8) + (y << 4) + z;
        }
        get(x, y, z) {
            const id = this.storage[this.index(x, y, z)];
            return this.palette[id];
        }
        set(x, y, z, value) {
            let id = this.palette.findIndex(b => b.equals(value));
            if (id === -1) {
                id = this.palette.length;
                this.palette.push(value);
            }
            this.storage[this.index(x, y, z)] = id;
        }
    }

    class ChunkSection {
        minY;
        static WIDTH = 16;
        static SIZE = ChunkSection.WIDTH * ChunkSection.WIDTH * ChunkSection.WIDTH;
        states;
        constructor(minY) {
            this.minY = minY;
            this.states = new PalettedContainer(ChunkSection.SIZE, BlockState.AIR);
        }
        get minBlockY() {
            return this.minY << 4;
        }
        getBlockState(x, y, z) {
            return this.states.get(x, y, z);
        }
        setBlockState(x, y, z, state) {
            this.states.set(x, y, z, state);
        }
    }

    var ChunkPos;
    (function (ChunkPos) {
        function create(x, z) {
            return [x, z];
        }
        ChunkPos.create = create;
        function fromBlockPos(blockPos) {
            return [blockPos[0] >> 4, blockPos[2] >> 4];
        }
        ChunkPos.fromBlockPos = fromBlockPos;
        function fromLong(long) {
            return [Number(long) & 0xFFFFFFFF, Number(long >> BigInt(32))];
        }
        ChunkPos.fromLong = fromLong;
        function toLong(chunkPos) {
            return asLong(chunkPos[0], chunkPos[1]);
        }
        ChunkPos.toLong = toLong;
        function asLong(x, z) {
            return BigInt(x & 0xFFFFFFFF) | BigInt(z & 0xFFFFFFFF) << BigInt(32);
        }
        ChunkPos.asLong = asLong;
        function minBlockX(chunkPos) {
            return chunkPos[0] << 4;
        }
        ChunkPos.minBlockX = minBlockX;
        function minBlockZ(chunkPos) {
            return chunkPos[1] << 4;
        }
        ChunkPos.minBlockZ = minBlockZ;
        function maxBlockX(chunkPos) {
            return (chunkPos[0] << 4) + 15;
        }
        ChunkPos.maxBlockX = maxBlockX;
        function maxBlockZ(chunkPos) {
            return (chunkPos[1] << 4) + 15;
        }
        ChunkPos.maxBlockZ = maxBlockZ;
    })(ChunkPos || (ChunkPos = {}));

    const EFFECT_COLORS = new Map([
        ['minecraft:speed', 3402751],
        ['minecraft:slowness', 9154528],
        ['minecraft:haste', 14270531],
        ['minecraft:mining_fatigue', 4866583],
        ['minecraft:strength', 16762624],
        ['minecraft:instant_health', 16262179],
        ['minecraft:instant_damage', 11101546],
        ['minecraft:jump_boost', 16646020],
        ['minecraft:nausea', 5578058],
        ['minecraft:regeneration', 13458603],
        ['minecraft:resistance', 9520880],
        ['minecraft:fire_resistance', 16750848],
        ['minecraft:water_breathing', 10017472],
        ['minecraft:invisibility', 16185078],
        ['minecraft:blindness', 2039587],
        ['minecraft:night_vision', 12779366],
        ['minecraft:hunger', 5797459],
        ['minecraft:weakness', 4738376],
        ['minecraft:poison', 8889187],
        ['minecraft:wither', 7561558],
        ['minecraft:health_boost', 16284963],
        ['minecraft:absorption', 2445989],
        ['minecraft:saturation', 16262179],
        ['minecraft:glowing', 9740385],
        ['minecraft:levitation', 13565951],
        ['minecraft:luck', 5882118],
        ['minecraft:unluck', 12624973],
        ['minecraft:slow_falling', 15978425],
        ['minecraft:conduit_power', 1950417],
        ['minecraft:dolphins_grace', 8954814],
        ['minecraft:bad_omen', 745784],
        ['minecraft:hero_of_the_village', 4521796],
        ['minecraft:darkness', 2696993],
        ['minecraft:trial_omen', 1484454],
        ['minecraft:raid_omen', 14565464],
        ['minecraft:wind_charged', 12438015],
        ['minecraft:weaving', 7891290],
        ['minecraft:oozing', 10092451],
        ['minecraft:infested', 9214860],
    ]);
    var MobEffectInstance;
    (function (MobEffectInstance) {
        function fromNbt(tag) {
            return {
                effect: Identifier.parse(tag.getString('id')),
                duration: tag.getNumber('duration'),
                amplifier: tag.getNumber('amplifier'),
            };
        }
        MobEffectInstance.fromNbt = fromNbt;
    })(MobEffectInstance || (MobEffectInstance = {}));
    const POTION_EFFECTS = new Map([
        ['minecraft:empty', []],
        ['minecraft:water', []],
        ['minecraft:mundane', []],
        ['minecraft:thick', []],
        ['minecraft:awkward', []],
        ['minecraft:night_vision', [{ effect: Identifier.create('night_vision'), duration: 3600, amplifier: 0 }]],
        ['minecraft:long_night_vision', [{ effect: Identifier.create('night_vision'), duration: 9600, amplifier: 0 }]],
        ['minecraft:invisibility', [{ effect: Identifier.create('invisibility'), duration: 3600, amplifier: 0 }]],
        ['minecraft:long_invisibility', [{ effect: Identifier.create('invisibility'), duration: 9600, amplifier: 0 }]],
        ['minecraft:leaping', [{ effect: Identifier.create('jump_boost'), duration: 3600, amplifier: 0 }]],
        ['minecraft:long_leaping', [{ effect: Identifier.create('jump_boost'), duration: 9600, amplifier: 0 }]],
        ['minecraft:strong_leaping', [{ effect: Identifier.create('jump_boost'), duration: 1800, amplifier: 1 }]],
        ['minecraft:fire_resistance', [{ effect: Identifier.create('fire_resistance'), duration: 3600, amplifier: 0 }]],
        ['minecraft:long_fire_resistance', [{ effect: Identifier.create('fire_resistance'), duration: 9600, amplifier: 0 }]],
        ['minecraft:swiftness', [{ effect: Identifier.create('speed'), duration: 3600, amplifier: 0 }]],
        ['minecraft:long_swiftness', [{ effect: Identifier.create('speed'), duration: 9600, amplifier: 0 }]],
        ['minecraft:strong_swiftness', [{ effect: Identifier.create('speed'), duration: 1800, amplifier: 1 }]],
        ['minecraft:slowness', [{ effect: Identifier.create('slowness'), duration: 1800, amplifier: 0 }]],
        ['minecraft:long_slowness', [{ effect: Identifier.create('slowness'), duration: 4800, amplifier: 0 }]],
        ['minecraft:strong_slowness', [{ effect: Identifier.create('slowness'), duration: 400, amplifier: 3 }]],
        ['minecraft:turtle_master', [{ effect: Identifier.create('slowness'), duration: 400, amplifier: 3 }, { effect: Identifier.create('resistance'), duration: 400, amplifier: 2 }]],
        ['minecraft:long_turtle_master', [{ effect: Identifier.create('slowness'), duration: 800, amplifier: 3 }, { effect: Identifier.create('resistance'), duration: 800, amplifier: 2 }]],
        ['minecraft:strong_turtle_master', [{ effect: Identifier.create('slowness'), duration: 400, amplifier: 5 }, { effect: Identifier.create('resistance'), duration: 400, amplifier: 3 }]],
        ['minecraft:water_breathing', [{ effect: Identifier.create('water_breathing'), duration: 3600, amplifier: 0 }]],
        ['minecraft:long_water_breathing', [{ effect: Identifier.create('water_breathing'), duration: 9600, amplifier: 0 }]],
        ['minecraft:healing', [{ effect: Identifier.create('instant_health'), duration: 1, amplifier: 0 }]],
        ['minecraft:strong_healing', [{ effect: Identifier.create('instant_health'), duration: 1, amplifier: 1 }]],
        ['minecraft:harming', [{ effect: Identifier.create('instant_damage'), duration: 1, amplifier: 0 }]],
        ['minecraft:strong_harming', [{ effect: Identifier.create('instant_damage'), duration: 1, amplifier: 1 }]],
        ['minecraft:poison', [{ effect: Identifier.create('poison'), duration: 900, amplifier: 0 }]],
        ['minecraft:long_poison', [{ effect: Identifier.create('poison'), duration: 1800, amplifier: 0 }]],
        ['minecraft:strong_poison', [{ effect: Identifier.create('poison'), duration: 432, amplifier: 1 }]],
        ['minecraft:regeneration', [{ effect: Identifier.create('regeneration'), duration: 900, amplifier: 0 }]],
        ['minecraft:long_regeneration', [{ effect: Identifier.create('regeneration'), duration: 1800, amplifier: 0 }]],
        ['minecraft:strong_regeneration', [{ effect: Identifier.create('regeneration'), duration: 450, amplifier: 1 }]],
        ['minecraft:strength', [{ effect: Identifier.create('strength'), duration: 3600, amplifier: 0 }]],
        ['minecraft:long_strength', [{ effect: Identifier.create('strength'), duration: 9600, amplifier: 0 }]],
        ['minecraft:strong_strength', [{ effect: Identifier.create('strength'), duration: 1800, amplifier: 1 }]],
        ['minecraft:weakness', [{ effect: Identifier.create('weakness'), duration: 1800, amplifier: 0 }]],
        ['minecraft:long_weakness', [{ effect: Identifier.create('weakness'), duration: 4800, amplifier: 0 }]],
        ['minecraft:luck', [{ effect: Identifier.create('luck'), duration: 6000, amplifier: 0 }]],
        ['minecraft:slow_falling', [{ effect: Identifier.create('slow_falling'), duration: 1800, amplifier: 0 }]],
        ['minecraft:long_slow_falling', [{ effect: Identifier.create('slow_falling'), duration: 4800, amplifier: 0 }]],
        ['minecraft:wind_charged', [{ effect: Identifier.create('wind_charged'), duration: 3600, amplifier: 0 }]],
        ['minecraft:weaving', [{ effect: Identifier.create('weaving'), duration: 3600, amplifier: 0 }]],
        ['minecraft:oozing', [{ effect: Identifier.create('oozing'), duration: 3600, amplifier: 0 }]],
        ['minecraft:infested', [{ effect: Identifier.create('infested'), duration: 3600, amplifier: 0 }]],
    ]);
    var PotionContents;
    (function (PotionContents) {
        function fromNbt(tag) {
            const ans = {};
            if (tag.isString()) {
                ans.potion = Identifier.parse(tag.getAsString());
            }
            else if (tag.isCompound()) {
                if (tag.hasString('potion')) {
                    ans.potion = Identifier.parse(tag.getString('potion'));
                }
                if (tag.hasNumber('custom_color')) {
                    ans.customColor = tag.getNumber('custom_color');
                }
                if (tag.hasList('custom_effects')) {
                    ans.customEffects = tag.getList('custom_effects', NbtType.Compound).map(MobEffectInstance.fromNbt);
                }
            }
            return ans;
        }
        PotionContents.fromNbt = fromNbt;
        function getColor(contents) {
            if (contents.customColor) {
                return Color.intToRgb(contents.customColor);
            }
            const effects = getAllEffects(contents);
            return mixEffectColors(effects);
        }
        PotionContents.getColor = getColor;
        function getAllEffects(contents) {
            const ans = [];
            if (contents.potion) {
                ans.push(...POTION_EFFECTS.get(contents.potion.toString()) ?? []);
            }
            if (contents.customEffects) {
                ans.push(...contents.customEffects);
            }
            return ans;
        }
        PotionContents.getAllEffects = getAllEffects;
        function mixEffectColors(effects) {
            let [r, g, b] = [0, 0, 0];
            let total = 0;
            for (const effect of effects) {
                const color = EFFECT_COLORS.get(effect.effect.toString());
                if (color === undefined)
                    continue;
                const rgb = Color.intToRgb(color);
                const amplifier = effect.amplifier + 1;
                r += amplifier * rgb[0];
                g += amplifier * rgb[1];
                b += amplifier * rgb[2];
                total += amplifier;
            }
            if (total === 0) {
                return Color.intToRgb(-13083194);
            }
            r = r / total;
            g = g / total;
            b = b / total;
            return [r, g, b];
        }
    })(PotionContents || (PotionContents = {}));

    var Holder;
    (function (Holder) {
        function parser(registry, directParser) {
            return (obj) => {
                if (typeof obj === 'string') {
                    return reference(registry, Identifier.parse(obj));
                }
                else {
                    return direct(directParser(obj));
                }
            };
        }
        Holder.parser = parser;
        function direct(value, id) {
            return {
                value: () => value,
                key: () => id,
            };
        }
        Holder.direct = direct;
        function reference(registry, id, required = true) {
            if (required) {
                return {
                    value: () => registry.getOrThrow(id),
                    key: () => id,
                };
            }
            else {
                return {
                    value: () => registry.get(id),
                    key: () => id,
                };
            }
        }
        Holder.reference = reference;
    })(Holder || (Holder = {}));

    class HolderSet {
        entries;
        constructor(entries) {
            this.entries = entries;
        }
        static parser(registry, valueParser) {
            const defaultedValueParser = valueParser ?? ((obj) => Holder.reference(registry, Identifier.parse(Json.readString(obj) ?? '')));
            return (obj) => {
                if (typeof obj === 'string') {
                    if (obj.startsWith('#')) {
                        return Holder.reference(registry.getTagRegistry(), Identifier.parse(obj.substring(1)));
                    }
                    else {
                        return Holder.direct(new HolderSet([]));
                    }
                }
                else {
                    return Holder.direct(new HolderSet(Json.readArray(obj, defaultedValueParser) ?? []) ?? []);
                }
            };
        }
        static fromJson(registry, obj, id) {
            const root = Json.readObject(obj) ?? {};
            const replace = Json.readBoolean(root.replace) ?? false;
            const entries = Json.readArray(root.values, (obj) => {
                var required = true;
                var id = '';
                if (typeof obj === 'string') {
                    id = obj;
                }
                else {
                    const entry = Json.readObject(obj) ?? {};
                    required = Json.readBoolean(entry.required) ?? false;
                    id = Json.readString(entry.id) ?? '';
                }
                if (id.startsWith('#')) {
                    return Holder.reference(registry.getTagRegistry(), Identifier.parse(id.substring(1)), required);
                }
                else {
                    return Holder.reference(registry, Identifier.parse(id), required);
                }
            }) ?? [];
            if (id && !replace && registry.getTagRegistry().has(id)) {
                entries?.push(Holder.direct(registry.getTagRegistry().get(id)));
            }
            return new HolderSet(entries);
        }
        *getEntries() {
            for (const entry of this.entries) {
                const value = entry.value();
                if (value === undefined) {
                    continue;
                }
                if (value instanceof HolderSet) {
                    yield* value.getEntries();
                }
                else {
                    yield entry;
                }
            }
        }
    }

    class ItemStack {
        id;
        count;
        components;
        constructor(id, count, components = new Map()) {
            this.id = id;
            this.count = count;
            this.components = components;
        }
        getComponent(key, baseComponents) {
            if (typeof key === 'string') {
                key = Identifier.parse(key);
            }
            if (this.components.has('!' + key.toString())) {
                return undefined;
            }
            const value = this.components.get(key.toString());
            if (value) {
                return value;
            }
            if (baseComponents) {
                return baseComponents.getItemComponents(this.id)?.get(key.toString());
            }
            return undefined;
        }
        hasComponent(key, baseComponents) {
            if (typeof key === 'string') {
                key = Identifier.parse(key);
            }
            if (this.components.has('!' + key.toString())) {
                return false;
            }
            if (this.components.has(key.toString())) {
                return true;
            }
            if (baseComponents) {
                return baseComponents.getItemComponents(this.id)?.has(key.toString());
            }
            return false;
        }
        clone() {
            // Component values are not cloned because they are assumed to be immutable
            const components = new Map(this.components);
            return new ItemStack(this.id, this.count, components);
        }
        is(other) {
            if (typeof other === 'string') {
                return this.id.equals(Identifier.parse(other));
            }
            if (other instanceof Identifier) {
                return this.id.equals(other);
            }
            return this.id.equals(other.id);
        }
        equals(other) {
            if (this === other) {
                return true;
            }
            if (!(other instanceof ItemStack)) {
                return false;
            }
            return this.count === other.count && this.isSameItemSameComponents(other);
        }
        isSameItemSameComponents(other) {
            if (!this.id.equals(other.id) || this.components.size !== other.components.size) {
                return false;
            }
            for (const [key, value] of this.components) {
                const otherValue = other.components.get(key);
                if (value.toString() !== otherValue?.toString()) {
                    return false;
                }
            }
            return true;
        }
        toString() {
            let result = this.id.toString();
            if (this.components.size > 0) {
                result += `[${[...this.components.entries()].map(([k, v]) => {
                return k.startsWith('!') ? k : `${k}=${v.toString()}`;
            }).join(',')}]`;
            }
            if (this.count > 1) {
                result += ` ${this.count}`;
            }
            return result;
        }
        static fromString(string) {
            const reader = new StringReader(string);
            while (reader.canRead() && reader.peek() !== '[') {
                reader.skip();
            }
            const itemId = Identifier.parse(reader.getRead());
            if (!reader.canRead()) {
                return new ItemStack(itemId, 1);
            }
            const components = new Map();
            reader.skip();
            if (reader.peek() === ']') {
                return new ItemStack(itemId, 1, components);
            }
            do {
                if (reader.peek() === '!') {
                    reader.skip();
                    reader.skipWhitespace();
                    const start = reader.cursor;
                    while (reader.canRead() && reader.peek() !== ']' && reader.peek() !== ',') {
                        reader.skip();
                    }
                    components.set('!' + Identifier.parse(reader.getRead(start).trim()).toString(), new NbtCompound());
                }
                else {
                    reader.skipWhitespace();
                    const start = reader.cursor;
                    while (reader.canRead() && reader.peek() !== '=') {
                        reader.skip();
                    }
                    const component = Identifier.parse(reader.getRead(start).trim()).toString();
                    if (!reader.canRead())
                        break;
                    reader.skip();
                    reader.skipWhitespace();
                    const tag = NbtParser.readTag(reader);
                    components.set(component, tag);
                }
                reader.skipWhitespace();
                if (!reader.canRead())
                    break;
                if (reader.peek() === ']') {
                    return new ItemStack(itemId, 1, components);
                }
                if (reader.peek() !== ',') {
                    throw new Error('Expected , or ]');
                }
                reader.skip();
            } while (reader.canRead());
            throw new Error('Missing closing ]');
        }
        toNbt() {
            const result = new NbtCompound()
                .set('id', new NbtString(this.id.toString()));
            if (this.count > 1) {
                result.set('count', new NbtInt(this.count));
            }
            if (this.components.size > 0) {
                result.set('components', new NbtCompound(this.components));
            }
            return result;
        }
        static fromNbt(nbt) {
            const id = Identifier.parse(nbt.getString('id'));
            const count = nbt.hasNumber('count') ? nbt.getNumber('count') : 1;
            const components = new Map(Object.entries(nbt.getCompound('components').map((key, value) => {
                if (key.startsWith('!')) {
                    return ['!' + Identifier.parse(key).toString(), new NbtCompound()];
                }
                return [Identifier.parse(key).toString(), value];
            })));
            return new ItemStack(id, count, components);
        }
    }

    class Registry {
        key;
        parser;
        static REGISTRY = new Registry(Identifier.create('root'));
        storage = new Map();
        builtin = new Map();
        tags = undefined;
        constructor(key, parser) {
            this.key = key;
            this.parser = parser;
        }
        static createAndRegister(name, parser) {
            const registry = new Registry(Identifier.create(name), parser);
            Registry.REGISTRY.register(registry.key, registry);
            return registry;
        }
        register(id, value, builtin) {
            this.storage.set(id.toString(), value);
            if (builtin) {
                this.builtin.set(id.toString(), value);
            }
            return Holder.reference(this, id);
        }
        delete(id) {
            const deleted = this.storage.delete(id.toString());
            this.builtin.delete(id.toString());
            return deleted;
        }
        keys() {
            return [...this.storage.keys()].map(e => Identifier.parse(e));
        }
        has(id) {
            return this.storage.has(id.toString());
        }
        get(id) {
            var value = this.storage.get(id.toString());
            if (value instanceof Function) {
                value = value();
                this.storage.set(id.toString(), value);
            }
            return value;
        }
        getOrThrow(id) {
            const value = this.get(id);
            if (value === undefined) {
                throw new Error(`Missing key in ${this.key.toString()}: ${id.toString()}`);
            }
            return value;
        }
        parse(obj) {
            if (!this.parser) {
                throw new Error(`No parser exists for ${this.key.toString()}`);
            }
            return this.parser(obj);
        }
        clear() {
            this.storage.clear();
            for (const [key, value] of this.builtin.entries()) {
                this.storage.set(key, value);
            }
            if (this.tags) {
                this.tags.clear();
            }
            return this;
        }
        assign(other) {
            if (!this.key.equals(other.key)) {
                throw new Error(`Cannot assign registry of type ${other.key.toString()} to registry of type ${this.key.toString()}`);
            }
            for (const key of other.keys()) {
                this.storage.set(key.toString(), other.getOrThrow(key));
            }
            return this;
        }
        cloneEmpty() {
            return new Registry(this.key, this.parser);
        }
        forEach(fn) {
            for (const [key, value] of this.storage.entries()) {
                fn(Identifier.parse(key), value instanceof Function ? value() : value, this);
            }
        }
        map(fn) {
            return [...this.storage.entries()].map(([key, value]) => {
                return fn(Identifier.parse(key), value instanceof Function ? value() : value, this);
            });
        }
        getTagRegistry() {
            if (this.tags === undefined) {
                this.tags = new Registry(new Identifier(this.key.namespace, `tags/${this.key.path}`));
            }
            return this.tags;
        }
    }

    var Rotation;
    (function (Rotation) {
        Rotation["NONE"] = "none";
        Rotation["CLOCKWISE_90"] = "clockwise_90";
        Rotation["CLOCKWISE_180"] = "180";
        Rotation["COUNTERCLOCKWISE_90"] = "counterclockwise_90";
    })(Rotation || (Rotation = {}));
    (function (Rotation) {
        function getRandom(random) {
            return [Rotation.NONE, Rotation.CLOCKWISE_90, Rotation.CLOCKWISE_180, Rotation.COUNTERCLOCKWISE_90][random.nextInt(4)];
        }
        Rotation.getRandom = getRandom;
    })(Rotation || (Rotation = {}));

    class Structure {
        size;
        palette;
        blocks;
        static REGISTRY = Registry.createAndRegister('structures');
        static EMPTY = new Structure(BlockPos.ZERO);
        blocksMap = [];
        constructor(size, palette = [], blocks = []) {
            this.size = size;
            this.palette = palette;
            this.blocks = blocks;
            blocks.forEach(block => {
                if (!this.isInside(block.pos)) {
                    throw new Error(`Found block at ${block.pos} which is outside the structure bounds ${this.size}`);
                }
                this.blocksMap[block.pos[0] * size[1] * size[2] + block.pos[1] * size[2] + block.pos[2]] = block;
            });
        }
        getSize() {
            return this.size;
        }
        addBlock(pos, name, properties, nbt) {
            if (!this.isInside(pos)) {
                throw new Error(`Cannot add block at ${pos} outside the structure bounds ${this.size}`);
            }
            const blockState = new BlockState(name, properties);
            let state = this.palette.findIndex(b => b.equals(blockState));
            if (state === -1) {
                state = this.palette.length;
                this.palette.push(blockState);
            }
            this.blocks.push({ pos, state, nbt });
            this.blocksMap[pos[0] * this.size[1] * this.size[2] + pos[1] * this.size[2] + pos[2]] = { pos, state, nbt };
            return this;
        }
        getBlocks() {
            return this.blocks.map(b => this.toPlacedBlock(b));
        }
        getBlock(pos) {
            if (!this.isInside(pos))
                return null;
            const block = this.blocksMap[pos[0] * this.size[1] * this.size[2] + pos[1] * this.size[2] + pos[2]];
            if (!block)
                return null;
            return this.toPlacedBlock(block);
        }
        toPlacedBlock(block) {
            const state = this.palette[block.state];
            if (!state) {
                throw new Error(`Block at ${block.pos.join(' ')} in structure references invalid palette index ${block.state}`);
            }
            return {
                pos: block.pos,
                state: state,
                nbt: block.nbt,
            };
        }
        isInside(pos) {
            return pos[0] >= 0 && pos[0] < this.size[0]
                && pos[1] >= 0 && pos[1] < this.size[1]
                && pos[2] >= 0 && pos[2] < this.size[2];
        }
        static fromNbt(nbt) {
            const size = BlockPos.fromNbt(nbt.getList('size'));
            const palette = nbt.getList('palette', NbtType.Compound).map(tag => BlockState.fromNbt(tag));
            const blocks = nbt.getList('blocks', NbtType.Compound).map(tag => {
                const pos = BlockPos.fromNbt(tag.getList('pos'));
                const state = tag.getNumber('state');
                const nbt = tag.getCompound('nbt');
                return { pos, state, nbt: nbt.size > 0 ? nbt : undefined };
            });
            return new Structure(size, palette, blocks);
        }
        static transform(pos, rotation, pivot) {
            switch (rotation) {
                case Rotation.COUNTERCLOCKWISE_90:
                    return BlockPos.create(pivot[0] - pivot[2] + pos[2], pos[1], pivot[0] + pivot[2] - pos[0]);
                case Rotation.CLOCKWISE_90:
                    return BlockPos.create(pivot[0] + pivot[2] - pos[2], pos[1], pivot[2] - pivot[0] + pos[0]);
                case Rotation.CLOCKWISE_180:
                    return BlockPos.create(pivot[0] + pivot[0] - pos[0], pos[1], pivot[2] + pivot[2] - pos[2]);
                default:
                    return pos;
            }
        }
    }

    const MIN_INT = -2147483648;
    const MAX_INT = 2147483647;
    const MIN_LONG = -9223372036854776000;
    const MAX_LONG = 9223372036854776000;
    function square(x) {
        return x * x;
    }
    function clamp$1(x, min, max) {
        return Math.max(min, Math.min(max, x));
    }
    function lerp(a, b, c) {
        return b + a * (c - b);
    }
    function floatLerp(a, b, c) {
        return Math.fround(b + Math.fround(a * Math.fround(c - b)));
    }
    function lerp2(a, b, c, d, e, f) {
        return lerp(b, lerp(a, c, d), lerp(a, e, f));
    }
    function lerp3(a, b, c, d, e, f, g, h, i, j, k) {
        return lerp(c, lerp2(a, b, d, e, f, g), lerp2(a, b, h, i, j, k));
    }
    function lazyLerp(a, b, c) {
        if (a === 0)
            return b();
        if (a === 1)
            return c();
        return b() + a * (c() - b());
    }
    function lazyLerp2(a, b, c, d, e, f) {
        return lazyLerp(b, () => lazyLerp(a, c, d), () => lazyLerp(a, e, f));
    }
    function lazyLerp3(a, b, c, d, e, f, g, h, i, j, k) {
        return lazyLerp(c, () => lazyLerp2(a, b, d, e, f, g), () => lazyLerp2(a, b, h, i, j, k));
    }
    function clampedLerp(a, b, c) {
        if (c < 0) {
            return a;
        }
        else if (c > 1) {
            return b;
        }
        else {
            return lerp(c, a, b);
        }
    }
    function inverseLerp(a, b, c) {
        return (a - b) / (c - b);
    }
    function smoothstep(x) {
        return x * x * x * (x * (x * 6 - 15) + 10);
    }
    function map(a, b, c, d, e) {
        return lerp(inverseLerp(a, b, c), d, e);
    }
    function clampedMap(a, b, c, d, e) {
        return clampedLerp(d, e, inverseLerp(a, b, c));
    }
    function intFloor(a) {
        return clamp$1(Math.floor(a), MIN_INT, MAX_INT);
    }
    function longFloor(a) {
        return clamp$1(Math.floor(a), MIN_LONG, MAX_LONG);
    }
    function binarySearch(n, n2, predicate) {
        let n3 = n2 - n;
        while (n3 > 0) {
            const n4 = Math.floor(n3 / 2);
            const n5 = n + n4;
            if (predicate(n5)) {
                n3 = n4;
                continue;
            }
            n = n5 + 1;
            n3 -= n4 + 1;
        }
        return n;
    }
    function getSeed(x, y, z) {
        let seed = BigInt(x * 3129871) ^ BigInt(z) * BigInt(116129781) ^ BigInt(y);
        seed = seed * seed * BigInt(42317861) + seed * BigInt(11);
        return seed >> BigInt(16);
    }
    function longfromBytes(a, b, c, d, e, f, g, h) {
        return BigInt(a) << BigInt(56)
            | BigInt(b) << BigInt(48)
            | BigInt(c) << BigInt(40)
            | BigInt(d) << BigInt(32)
            | BigInt(e) << BigInt(24)
            | BigInt(f) << BigInt(16)
            | BigInt(g) << BigInt(8)
            | BigInt(h);
    }
    function isPowerOfTwo(x) {
        return (x & (x - 1)) === 0;
    }
    function upperPowerOfTwo(x) {
        x -= 1;
        x |= x >> 1;
        x |= x >> 2;
        x |= x >> 4;
        x |= x >> 8;
        x |= x >> 18;
        x |= x >> 32;
        return x + 1;
    }
    function randomBetweenInclusive(random, min, max) {
        return random.nextInt(max - min + 1) + min;
    }
    function nextInt(random, min, max) {
        return min >= max ? min : random.nextInt(max - min + 1) + min;
    }
    function shuffle(array, random) {
        for (var i = array.length; i > 1; i--) {
            const switchIndex = random.nextInt(i);
            const tmp = array[switchIndex];
            array[switchIndex] = array[i - 1];
            array[i - 1] = tmp;
        }
    }

    var MinMaxNumberFunction;
    (function (MinMaxNumberFunction) {
        function is(obj) {
            return typeof obj === 'object' && obj !== null && 'minValue' in obj && 'maxValue' in obj;
        }
        MinMaxNumberFunction.is = is;
    })(MinMaxNumberFunction || (MinMaxNumberFunction = {}));
    var CubicSpline;
    (function (CubicSpline) {
        function fromJson(obj, extractor) {
            if (typeof obj === 'number') {
                return new Constant(obj);
            }
            const root = Json.readObject(obj) ?? {};
            const spline = new MultiPoint(extractor(root.coordinate));
            const points = Json.readArray(root.points, e => Json.readObject(e) ?? {}) ?? [];
            if (points.length === 0) {
                return new Constant(0);
            }
            for (const point of points) {
                const location = Json.readNumber(point.location) ?? 0;
                const value = fromJson(point.value, extractor);
                const derivative = Json.readNumber(point.derivative) ?? 0;
                spline.addPoint(location, value, derivative);
            }
            return spline;
        }
        CubicSpline.fromJson = fromJson;
        class Constant {
            value;
            constructor(value) {
                this.value = value;
            }
            compute() {
                return this.value;
            }
            min() {
                return this.value;
            }
            max() {
                return this.value;
            }
            mapAll() {
                return this;
            }
            calculateMinMax() { }
        }
        CubicSpline.Constant = Constant;
        class MultiPoint {
            coordinate;
            locations;
            values;
            derivatives;
            calculatedMin = Number.NEGATIVE_INFINITY;
            calculatedMax = Number.POSITIVE_INFINITY;
            constructor(coordinate, locations = [], values = [], derivatives = []) {
                this.coordinate = coordinate;
                this.locations = locations;
                this.values = values;
                this.derivatives = derivatives;
            }
            compute(c) {
                const coordinate = this.coordinate.compute(c);
                const i = binarySearch(0, this.locations.length, n => coordinate < this.locations[n]) - 1;
                const n = this.locations.length - 1;
                // TODO: use linear extend for this 
                if (i < 0) {
                    return Math.fround(this.values[0].compute(c) + Math.fround(this.derivatives[0] * Math.fround(coordinate - this.locations[0])));
                }
                if (i === n) {
                    return Math.fround(this.values[n].compute(c) + Math.fround(this.derivatives[n] * Math.fround(coordinate - this.locations[n])));
                }
                const loc0 = this.locations[i];
                const loc1 = this.locations[i + 1];
                const der0 = this.derivatives[i];
                const der1 = this.derivatives[i + 1];
                const f = Math.fround(Math.fround(coordinate - loc0) / Math.fround(loc1 - loc0));
                const val0 = this.values[i].compute(c);
                const val1 = this.values[i + 1].compute(c);
                const f8 = Math.fround(Math.fround(der0 * Math.fround(loc1 - loc0)) - Math.fround(val1 - val0));
                const f9 = Math.fround(Math.fround(-der1 * Math.fround(loc1 - loc0)) + Math.fround(val1 - val0));
                const f10 = Math.fround(floatLerp(f, val0, val1) + Math.fround(Math.fround(f * Math.fround(1.0 - f)) * floatLerp(f, f8, f9)));
                return f10;
            }
            min() {
                return this.calculatedMin;
            }
            max() {
                return this.calculatedMax;
            }
            mapAll(visitor) {
                return new MultiPoint(visitor(this.coordinate), this.locations, this.values.map(v => v.mapAll(visitor)), this.derivatives);
            }
            addPoint(location, value, derivative = 0) {
                this.locations.push(Math.fround(location));
                this.values.push(typeof value === 'number'
                    ? new CubicSpline.Constant(Math.fround(value))
                    : value);
                this.derivatives.push(Math.fround(derivative));
                return this;
            }
            calculateMinMax() {
                if (!MinMaxNumberFunction.is(this.coordinate)) {
                    return;
                }
                const lastIdx = this.locations.length - 1;
                var splineMin = Number.POSITIVE_INFINITY;
                var splineMax = Number.NEGATIVE_INFINITY;
                const coordinateMin = this.coordinate.minValue();
                const coordinateMax = this.coordinate.maxValue();
                for (const innerSpline of this.values) {
                    innerSpline.calculateMinMax();
                }
                if (coordinateMin < this.locations[0]) {
                    const minExtend = MultiPoint.linearExtend(coordinateMin, this.locations, (this.values[0]).min(), this.derivatives, 0);
                    const maxExtend = MultiPoint.linearExtend(coordinateMin, this.locations, (this.values[0]).max(), this.derivatives, 0);
                    splineMin = Math.min(splineMin, Math.min(minExtend, maxExtend));
                    splineMax = Math.max(splineMax, Math.max(minExtend, maxExtend));
                }
                if (coordinateMax > this.locations[lastIdx]) {
                    const minExtend = MultiPoint.linearExtend(coordinateMax, this.locations, (this.values[lastIdx]).min(), this.derivatives, lastIdx);
                    const maxExtend = MultiPoint.linearExtend(coordinateMax, this.locations, (this.values[lastIdx]).max(), this.derivatives, lastIdx);
                    splineMin = Math.min(splineMin, Math.min(minExtend, maxExtend));
                    splineMax = Math.max(splineMax, Math.max(minExtend, maxExtend));
                }
                for (const innerSpline of this.values) {
                    splineMin = Math.min(splineMin, innerSpline.min());
                    splineMax = Math.max(splineMax, innerSpline.max());
                }
                for (var i = 0; i < lastIdx; ++i) {
                    const locationLeft = this.locations[i];
                    const locationRight = this.locations[i + 1];
                    const locationDelta = Math.fround(locationRight - locationLeft);
                    const splineLeft = this.values[i];
                    const splineRight = this.values[i + 1];
                    const minLeft = splineLeft.min();
                    const maxLeft = splineLeft.max();
                    const minRight = splineRight.min();
                    const maxRight = splineRight.max();
                    const derivativeLeft = this.derivatives[i];
                    const derivativeRight = this.derivatives[i + 1];
                    if (derivativeLeft !== 0.0 || derivativeRight !== 0.0) {
                        const maxValueDeltaLeft = Math.fround(derivativeLeft * locationDelta);
                        const maxValueDeltaRight = Math.fround(derivativeRight * locationDelta);
                        const minValue = Math.min(minLeft, minRight);
                        const maxValue = Math.max(maxLeft, maxRight);
                        const minDeltaLeft = Math.fround(Math.fround(maxValueDeltaLeft - maxRight) + minLeft);
                        const maxDeltaLeft = Math.fround(Math.fround(maxValueDeltaLeft - minRight) + maxLeft);
                        const minDeltaRight = Math.fround(Math.fround(-maxValueDeltaRight + minRight) - maxLeft);
                        const maxDeltaRight = Math.fround(Math.fround(-maxValueDeltaRight + maxRight) - minLeft);
                        const minDelta = Math.min(minDeltaLeft, minDeltaRight);
                        const maxDelta = Math.max(maxDeltaLeft, maxDeltaRight);
                        splineMin = Math.min(splineMin, Math.fround(minValue + Math.fround(0.25 * minDelta)));
                        splineMax = Math.max(splineMax, Math.fround(maxValue + Math.fround(0.25 * maxDelta)));
                    }
                }
                this.calculatedMin = splineMin;
                this.calculatedMax = splineMax;
            }
            static linearExtend(location, locations, value, derivatives, useIndex) {
                const derivative = derivatives[useIndex];
                if (derivative == 0) {
                    return value;
                }
                return Math.fround(value + Math.fround(derivative * Math.fround(location - locations[useIndex])));
            }
        }
        CubicSpline.MultiPoint = MultiPoint;
    })(CubicSpline || (CubicSpline = {}));

    /**
     * Common utilities
     * @module glMatrix
     */
    // Configuration Constants
    var EPSILON = 0.000001;
    var ARRAY_TYPE = typeof Float32Array !== 'undefined' ? Float32Array : Array;
    var degree = Math.PI / 180;
    /**
     * Convert Degree To Radian
     *
     * @param {Number} a Angle in Degrees
     */

    function toRadian(a) {
      return a * degree;
    }
    if (!Math.hypot) Math.hypot = function () {
      var y = 0,
          i = arguments.length;

      while (i--) {
        y += arguments[i] * arguments[i];
      }

      return Math.sqrt(y);
    };

    /**
     * 4x4 Matrix<br>Format: column-major, when typed out it looks like row-major<br>The matrices are being post multiplied.
     * @module mat4
     */

    /**
     * Creates a new identity mat4
     *
     * @returns {mat4} a new 4x4 matrix
     */

    function create$2() {
      var out = new ARRAY_TYPE(16);

      if (ARRAY_TYPE != Float32Array) {
        out[1] = 0;
        out[2] = 0;
        out[3] = 0;
        out[4] = 0;
        out[6] = 0;
        out[7] = 0;
        out[8] = 0;
        out[9] = 0;
        out[11] = 0;
        out[12] = 0;
        out[13] = 0;
        out[14] = 0;
      }

      out[0] = 1;
      out[5] = 1;
      out[10] = 1;
      out[15] = 1;
      return out;
    }
    /**
     * Copy the values from one mat4 to another
     *
     * @param {mat4} out the receiving matrix
     * @param {ReadonlyMat4} a the source matrix
     * @returns {mat4} out
     */

    function copy$1(out, a) {
      out[0] = a[0];
      out[1] = a[1];
      out[2] = a[2];
      out[3] = a[3];
      out[4] = a[4];
      out[5] = a[5];
      out[6] = a[6];
      out[7] = a[7];
      out[8] = a[8];
      out[9] = a[9];
      out[10] = a[10];
      out[11] = a[11];
      out[12] = a[12];
      out[13] = a[13];
      out[14] = a[14];
      out[15] = a[15];
      return out;
    }
    /**
     * Translate a mat4 by the given vector
     *
     * @param {mat4} out the receiving matrix
     * @param {ReadonlyMat4} a the matrix to translate
     * @param {ReadonlyVec3} v vector to translate by
     * @returns {mat4} out
     */

    function translate(out, a, v) {
      var x = v[0],
          y = v[1],
          z = v[2];
      var a00, a01, a02, a03;
      var a10, a11, a12, a13;
      var a20, a21, a22, a23;

      if (a === out) {
        out[12] = a[0] * x + a[4] * y + a[8] * z + a[12];
        out[13] = a[1] * x + a[5] * y + a[9] * z + a[13];
        out[14] = a[2] * x + a[6] * y + a[10] * z + a[14];
        out[15] = a[3] * x + a[7] * y + a[11] * z + a[15];
      } else {
        a00 = a[0];
        a01 = a[1];
        a02 = a[2];
        a03 = a[3];
        a10 = a[4];
        a11 = a[5];
        a12 = a[6];
        a13 = a[7];
        a20 = a[8];
        a21 = a[9];
        a22 = a[10];
        a23 = a[11];
        out[0] = a00;
        out[1] = a01;
        out[2] = a02;
        out[3] = a03;
        out[4] = a10;
        out[5] = a11;
        out[6] = a12;
        out[7] = a13;
        out[8] = a20;
        out[9] = a21;
        out[10] = a22;
        out[11] = a23;
        out[12] = a00 * x + a10 * y + a20 * z + a[12];
        out[13] = a01 * x + a11 * y + a21 * z + a[13];
        out[14] = a02 * x + a12 * y + a22 * z + a[14];
        out[15] = a03 * x + a13 * y + a23 * z + a[15];
      }

      return out;
    }
    /**
     * Scales the mat4 by the dimensions in the given vec3 not using vectorization
     *
     * @param {mat4} out the receiving matrix
     * @param {ReadonlyMat4} a the matrix to scale
     * @param {ReadonlyVec3} v the vec3 to scale the matrix by
     * @returns {mat4} out
     **/

    function scale$1(out, a, v) {
      var x = v[0],
          y = v[1],
          z = v[2];
      out[0] = a[0] * x;
      out[1] = a[1] * x;
      out[2] = a[2] * x;
      out[3] = a[3] * x;
      out[4] = a[4] * y;
      out[5] = a[5] * y;
      out[6] = a[6] * y;
      out[7] = a[7] * y;
      out[8] = a[8] * z;
      out[9] = a[9] * z;
      out[10] = a[10] * z;
      out[11] = a[11] * z;
      out[12] = a[12];
      out[13] = a[13];
      out[14] = a[14];
      out[15] = a[15];
      return out;
    }
    /**
     * Rotates a mat4 by the given angle around the given axis
     *
     * @param {mat4} out the receiving matrix
     * @param {ReadonlyMat4} a the matrix to rotate
     * @param {Number} rad the angle to rotate the matrix by
     * @param {ReadonlyVec3} axis the axis to rotate around
     * @returns {mat4} out
     */

    function rotate(out, a, rad, axis) {
      var x = axis[0],
          y = axis[1],
          z = axis[2];
      var len = Math.hypot(x, y, z);
      var s, c, t;
      var a00, a01, a02, a03;
      var a10, a11, a12, a13;
      var a20, a21, a22, a23;
      var b00, b01, b02;
      var b10, b11, b12;
      var b20, b21, b22;

      if (len < EPSILON) {
        return null;
      }

      len = 1 / len;
      x *= len;
      y *= len;
      z *= len;
      s = Math.sin(rad);
      c = Math.cos(rad);
      t = 1 - c;
      a00 = a[0];
      a01 = a[1];
      a02 = a[2];
      a03 = a[3];
      a10 = a[4];
      a11 = a[5];
      a12 = a[6];
      a13 = a[7];
      a20 = a[8];
      a21 = a[9];
      a22 = a[10];
      a23 = a[11]; // Construct the elements of the rotation matrix

      b00 = x * x * t + c;
      b01 = y * x * t + z * s;
      b02 = z * x * t - y * s;
      b10 = x * y * t - z * s;
      b11 = y * y * t + c;
      b12 = z * y * t + x * s;
      b20 = x * z * t + y * s;
      b21 = y * z * t - x * s;
      b22 = z * z * t + c; // Perform rotation-specific matrix multiplication

      out[0] = a00 * b00 + a10 * b01 + a20 * b02;
      out[1] = a01 * b00 + a11 * b01 + a21 * b02;
      out[2] = a02 * b00 + a12 * b01 + a22 * b02;
      out[3] = a03 * b00 + a13 * b01 + a23 * b02;
      out[4] = a00 * b10 + a10 * b11 + a20 * b12;
      out[5] = a01 * b10 + a11 * b11 + a21 * b12;
      out[6] = a02 * b10 + a12 * b11 + a22 * b12;
      out[7] = a03 * b10 + a13 * b11 + a23 * b12;
      out[8] = a00 * b20 + a10 * b21 + a20 * b22;
      out[9] = a01 * b20 + a11 * b21 + a21 * b22;
      out[10] = a02 * b20 + a12 * b21 + a22 * b22;
      out[11] = a03 * b20 + a13 * b21 + a23 * b22;

      if (a !== out) {
        // If the source and destination differ, copy the unchanged last row
        out[12] = a[12];
        out[13] = a[13];
        out[14] = a[14];
        out[15] = a[15];
      }

      return out;
    }
    /**
     * Rotates a matrix by the given angle around the X axis
     *
     * @param {mat4} out the receiving matrix
     * @param {ReadonlyMat4} a the matrix to rotate
     * @param {Number} rad the angle to rotate the matrix by
     * @returns {mat4} out
     */

    function rotateX$1(out, a, rad) {
      var s = Math.sin(rad);
      var c = Math.cos(rad);
      var a10 = a[4];
      var a11 = a[5];
      var a12 = a[6];
      var a13 = a[7];
      var a20 = a[8];
      var a21 = a[9];
      var a22 = a[10];
      var a23 = a[11];

      if (a !== out) {
        // If the source and destination differ, copy the unchanged rows
        out[0] = a[0];
        out[1] = a[1];
        out[2] = a[2];
        out[3] = a[3];
        out[12] = a[12];
        out[13] = a[13];
        out[14] = a[14];
        out[15] = a[15];
      } // Perform axis-specific matrix multiplication


      out[4] = a10 * c + a20 * s;
      out[5] = a11 * c + a21 * s;
      out[6] = a12 * c + a22 * s;
      out[7] = a13 * c + a23 * s;
      out[8] = a20 * c - a10 * s;
      out[9] = a21 * c - a11 * s;
      out[10] = a22 * c - a12 * s;
      out[11] = a23 * c - a13 * s;
      return out;
    }
    /**
     * Rotates a matrix by the given angle around the Y axis
     *
     * @param {mat4} out the receiving matrix
     * @param {ReadonlyMat4} a the matrix to rotate
     * @param {Number} rad the angle to rotate the matrix by
     * @returns {mat4} out
     */

    function rotateY$1(out, a, rad) {
      var s = Math.sin(rad);
      var c = Math.cos(rad);
      var a00 = a[0];
      var a01 = a[1];
      var a02 = a[2];
      var a03 = a[3];
      var a20 = a[8];
      var a21 = a[9];
      var a22 = a[10];
      var a23 = a[11];

      if (a !== out) {
        // If the source and destination differ, copy the unchanged rows
        out[4] = a[4];
        out[5] = a[5];
        out[6] = a[6];
        out[7] = a[7];
        out[12] = a[12];
        out[13] = a[13];
        out[14] = a[14];
        out[15] = a[15];
      } // Perform axis-specific matrix multiplication


      out[0] = a00 * c - a20 * s;
      out[1] = a01 * c - a21 * s;
      out[2] = a02 * c - a22 * s;
      out[3] = a03 * c - a23 * s;
      out[8] = a00 * s + a20 * c;
      out[9] = a01 * s + a21 * c;
      out[10] = a02 * s + a22 * c;
      out[11] = a03 * s + a23 * c;
      return out;
    }
    /**
     * Rotates a matrix by the given angle around the Z axis
     *
     * @param {mat4} out the receiving matrix
     * @param {ReadonlyMat4} a the matrix to rotate
     * @param {Number} rad the angle to rotate the matrix by
     * @returns {mat4} out
     */

    function rotateZ(out, a, rad) {
      var s = Math.sin(rad);
      var c = Math.cos(rad);
      var a00 = a[0];
      var a01 = a[1];
      var a02 = a[2];
      var a03 = a[3];
      var a10 = a[4];
      var a11 = a[5];
      var a12 = a[6];
      var a13 = a[7];

      if (a !== out) {
        // If the source and destination differ, copy the unchanged last row
        out[8] = a[8];
        out[9] = a[9];
        out[10] = a[10];
        out[11] = a[11];
        out[12] = a[12];
        out[13] = a[13];
        out[14] = a[14];
        out[15] = a[15];
      } // Perform axis-specific matrix multiplication


      out[0] = a00 * c + a10 * s;
      out[1] = a01 * c + a11 * s;
      out[2] = a02 * c + a12 * s;
      out[3] = a03 * c + a13 * s;
      out[4] = a10 * c - a00 * s;
      out[5] = a11 * c - a01 * s;
      out[6] = a12 * c - a02 * s;
      out[7] = a13 * c - a03 * s;
      return out;
    }
    /**
     * Generates a perspective projection matrix with the given bounds.
     * The near/far clip planes correspond to a normalized device coordinate Z range of [-1, 1],
     * which matches WebGL/OpenGL's clip volume.
     * Passing null/undefined/no value for far will generate infinite projection matrix.
     *
     * @param {mat4} out mat4 frustum matrix will be written into
     * @param {number} fovy Vertical field of view in radians
     * @param {number} aspect Aspect ratio. typically viewport width/height
     * @param {number} near Near bound of the frustum
     * @param {number} far Far bound of the frustum, can be null or Infinity
     * @returns {mat4} out
     */

    function perspectiveNO(out, fovy, aspect, near, far) {
      var f = 1.0 / Math.tan(fovy / 2),
          nf;
      out[0] = f / aspect;
      out[1] = 0;
      out[2] = 0;
      out[3] = 0;
      out[4] = 0;
      out[5] = f;
      out[6] = 0;
      out[7] = 0;
      out[8] = 0;
      out[9] = 0;
      out[11] = -1;
      out[12] = 0;
      out[13] = 0;
      out[15] = 0;

      if (far != null && far !== Infinity) {
        nf = 1 / (near - far);
        out[10] = (far + near) * nf;
        out[14] = 2 * far * near * nf;
      } else {
        out[10] = -1;
        out[14] = -2 * near;
      }

      return out;
    }
    /**
     * Alias for {@link mat4.perspectiveNO}
     * @function
     */

    var perspective = perspectiveNO;
    /**
     * Generates a orthogonal projection matrix with the given bounds.
     * The near/far clip planes correspond to a normalized device coordinate Z range of [-1, 1],
     * which matches WebGL/OpenGL's clip volume.
     *
     * @param {mat4} out mat4 frustum matrix will be written into
     * @param {number} left Left bound of the frustum
     * @param {number} right Right bound of the frustum
     * @param {number} bottom Bottom bound of the frustum
     * @param {number} top Top bound of the frustum
     * @param {number} near Near bound of the frustum
     * @param {number} far Far bound of the frustum
     * @returns {mat4} out
     */

    function orthoNO(out, left, right, bottom, top, near, far) {
      var lr = 1 / (left - right);
      var bt = 1 / (bottom - top);
      var nf = 1 / (near - far);
      out[0] = -2 * lr;
      out[1] = 0;
      out[2] = 0;
      out[3] = 0;
      out[4] = 0;
      out[5] = -2 * bt;
      out[6] = 0;
      out[7] = 0;
      out[8] = 0;
      out[9] = 0;
      out[10] = 2 * nf;
      out[11] = 0;
      out[12] = (left + right) * lr;
      out[13] = (top + bottom) * bt;
      out[14] = (far + near) * nf;
      out[15] = 1;
      return out;
    }
    /**
     * Alias for {@link mat4.orthoNO}
     * @function
     */

    var ortho = orthoNO;

    /**
     * 3 Dimensional Vector
     * @module vec3
     */

    /**
     * Creates a new, empty vec3
     *
     * @returns {vec3} a new 3D vector
     */

    function create$1() {
      var out = new ARRAY_TYPE(3);

      if (ARRAY_TYPE != Float32Array) {
        out[0] = 0;
        out[1] = 0;
        out[2] = 0;
      }

      return out;
    }
    /**
     * Creates a new vec3 initialized with the given values
     *
     * @param {Number} x X component
     * @param {Number} y Y component
     * @param {Number} z Z component
     * @returns {vec3} a new 3D vector
     */

    function fromValues$1(x, y, z) {
      var out = new ARRAY_TYPE(3);
      out[0] = x;
      out[1] = y;
      out[2] = z;
      return out;
    }
    /**
     * Copy the values from one vec3 to another
     *
     * @param {vec3} out the receiving vector
     * @param {ReadonlyVec3} a the source vector
     * @returns {vec3} out
     */

    function copy(out, a) {
      out[0] = a[0];
      out[1] = a[1];
      out[2] = a[2];
      return out;
    }
    /**
     * Adds two vec3's
     *
     * @param {vec3} out the receiving vector
     * @param {ReadonlyVec3} a the first operand
     * @param {ReadonlyVec3} b the second operand
     * @returns {vec3} out
     */

    function add$1(out, a, b) {
      out[0] = a[0] + b[0];
      out[1] = a[1] + b[1];
      out[2] = a[2] + b[2];
      return out;
    }
    /**
     * Multiplies two vec3's
     *
     * @param {vec3} out the receiving vector
     * @param {ReadonlyVec3} a the first operand
     * @param {ReadonlyVec3} b the second operand
     * @returns {vec3} out
     */

    function multiply(out, a, b) {
      out[0] = a[0] * b[0];
      out[1] = a[1] * b[1];
      out[2] = a[2] * b[2];
      return out;
    }
    /**
     * Scales a vec3 by a scalar number
     *
     * @param {vec3} out the receiving vector
     * @param {ReadonlyVec3} a the vector to scale
     * @param {Number} b amount to scale the vector by
     * @returns {vec3} out
     */

    function scale(out, a, b) {
      out[0] = a[0] * b;
      out[1] = a[1] * b;
      out[2] = a[2] * b;
      return out;
    }
    /**
     * Adds two vec3's after scaling the second operand by a scalar value
     *
     * @param {vec3} out the receiving vector
     * @param {ReadonlyVec3} a the first operand
     * @param {ReadonlyVec3} b the second operand
     * @param {Number} scale the amount to scale b by before adding
     * @returns {vec3} out
     */

    function scaleAndAdd(out, a, b, scale) {
      out[0] = a[0] + b[0] * scale;
      out[1] = a[1] + b[1] * scale;
      out[2] = a[2] + b[2] * scale;
      return out;
    }
    /**
     * Calculates the euclidian distance between two vec3's
     *
     * @param {ReadonlyVec3} a the first operand
     * @param {ReadonlyVec3} b the second operand
     * @returns {Number} distance between a and b
     */

    function distance(a, b) {
      var x = b[0] - a[0];
      var y = b[1] - a[1];
      var z = b[2] - a[2];
      return Math.hypot(x, y, z);
    }
    /**
     * Negates the components of a vec3
     *
     * @param {vec3} out the receiving vector
     * @param {ReadonlyVec3} a vector to negate
     * @returns {vec3} out
     */

    function negate(out, a) {
      out[0] = -a[0];
      out[1] = -a[1];
      out[2] = -a[2];
      return out;
    }
    /**
     * Transforms the vec3 with a mat4.
     * 4th vector component is implicitly '1'
     *
     * @param {vec3} out the receiving vector
     * @param {ReadonlyVec3} a the vector to transform
     * @param {ReadonlyMat4} m matrix to transform with
     * @returns {vec3} out
     */

    function transformMat4(out, a, m) {
      var x = a[0],
          y = a[1],
          z = a[2];
      var w = m[3] * x + m[7] * y + m[11] * z + m[15];
      w = w || 1.0;
      out[0] = (m[0] * x + m[4] * y + m[8] * z + m[12]) / w;
      out[1] = (m[1] * x + m[5] * y + m[9] * z + m[13]) / w;
      out[2] = (m[2] * x + m[6] * y + m[10] * z + m[14]) / w;
      return out;
    }
    /**
     * Rotate a 3D vector around the x-axis
     * @param {vec3} out The receiving vec3
     * @param {ReadonlyVec3} a The vec3 point to rotate
     * @param {ReadonlyVec3} b The origin of the rotation
     * @param {Number} rad The angle of rotation in radians
     * @returns {vec3} out
     */

    function rotateX(out, a, b, rad) {
      var p = [],
          r = []; //Translate point to the origin

      p[0] = a[0] - b[0];
      p[1] = a[1] - b[1];
      p[2] = a[2] - b[2]; //perform rotation

      r[0] = p[0];
      r[1] = p[1] * Math.cos(rad) - p[2] * Math.sin(rad);
      r[2] = p[1] * Math.sin(rad) + p[2] * Math.cos(rad); //translate to correct position

      out[0] = r[0] + b[0];
      out[1] = r[1] + b[1];
      out[2] = r[2] + b[2];
      return out;
    }
    /**
     * Rotate a 3D vector around the y-axis
     * @param {vec3} out The receiving vec3
     * @param {ReadonlyVec3} a The vec3 point to rotate
     * @param {ReadonlyVec3} b The origin of the rotation
     * @param {Number} rad The angle of rotation in radians
     * @returns {vec3} out
     */

    function rotateY(out, a, b, rad) {
      var p = [],
          r = []; //Translate point to the origin

      p[0] = a[0] - b[0];
      p[1] = a[1] - b[1];
      p[2] = a[2] - b[2]; //perform rotation

      r[0] = p[2] * Math.sin(rad) + p[0] * Math.cos(rad);
      r[1] = p[1];
      r[2] = p[2] * Math.cos(rad) - p[0] * Math.sin(rad); //translate to correct position

      out[0] = r[0] + b[0];
      out[1] = r[1] + b[1];
      out[2] = r[2] + b[2];
      return out;
    }
    /**
     * Returns whether or not the vectors have approximately the same elements in the same position.
     *
     * @param {ReadonlyVec3} a The first vector.
     * @param {ReadonlyVec3} b The second vector.
     * @returns {Boolean} True if the vectors are equal, false otherwise.
     */

    function equals(a, b) {
      var a0 = a[0],
          a1 = a[1],
          a2 = a[2];
      var b0 = b[0],
          b1 = b[1],
          b2 = b[2];
      return Math.abs(a0 - b0) <= EPSILON * Math.max(1.0, Math.abs(a0), Math.abs(b0)) && Math.abs(a1 - b1) <= EPSILON * Math.max(1.0, Math.abs(a1), Math.abs(b1)) && Math.abs(a2 - b2) <= EPSILON * Math.max(1.0, Math.abs(a2), Math.abs(b2));
    }
    /**
     * Alias for {@link vec3.multiply}
     * @function
     */

    var mul = multiply;
    /**
     * Alias for {@link vec3.distance}
     * @function
     */

    var dist = distance;
    /**
     * Perform some operation over an array of vec3s.
     *
     * @param {Array} a the array of vectors to iterate over
     * @param {Number} stride Number of elements between the start of each vec3. If 0 assumes tightly packed
     * @param {Number} offset Number of elements to skip at the beginning of the array
     * @param {Number} count Number of vec3s to iterate over. If 0 iterates over entire array
     * @param {Function} fn Function to call for each vector in the array
     * @param {Object} [arg] additional argument to pass to fn
     * @returns {Array} a
     * @function
     */

    (function () {
      var vec = create$1();
      return function (a, stride, offset, count, fn, arg) {
        var i, l;

        if (!stride) {
          stride = 3;
        }

        if (!offset) {
          offset = 0;
        }

        if (count) {
          l = Math.min(count * stride + offset, a.length);
        } else {
          l = a.length;
        }

        for (i = offset; i < l; i += stride) {
          vec[0] = a[i];
          vec[1] = a[i + 1];
          vec[2] = a[i + 2];
          fn(vec, vec, arg);
          a[i] = vec[0];
          a[i + 1] = vec[1];
          a[i + 2] = vec[2];
        }

        return a;
      };
    })();

    /**
     * 2 Dimensional Vector
     * @module vec2
     */

    /**
     * Creates a new, empty vec2
     *
     * @returns {vec2} a new 2D vector
     */

    function create() {
      var out = new ARRAY_TYPE(2);

      if (ARRAY_TYPE != Float32Array) {
        out[0] = 0;
        out[1] = 0;
      }

      return out;
    }
    /**
     * Creates a new vec2 initialized with the given values
     *
     * @param {Number} x X component
     * @param {Number} y Y component
     * @returns {vec2} a new 2D vector
     */

    function fromValues(x, y) {
      var out = new ARRAY_TYPE(2);
      out[0] = x;
      out[1] = y;
      return out;
    }
    /**
     * Adds two vec2's
     *
     * @param {vec2} out the receiving vector
     * @param {ReadonlyVec2} a the first operand
     * @param {ReadonlyVec2} b the second operand
     * @returns {vec2} out
     */

    function add(out, a, b) {
      out[0] = a[0] + b[0];
      out[1] = a[1] + b[1];
      return out;
    }
    /**
     * Perform some operation over an array of vec2s.
     *
     * @param {Array} a the array of vectors to iterate over
     * @param {Number} stride Number of elements between the start of each vec2. If 0 assumes tightly packed
     * @param {Number} offset Number of elements to skip at the beginning of the array
     * @param {Number} count Number of vec2s to iterate over. If 0 iterates over entire array
     * @param {Function} fn Function to call for each vector in the array
     * @param {Object} [arg] additional argument to pass to fn
     * @returns {Array} a
     * @function
     */

    (function () {
      var vec = create();
      return function (a, stride, offset, count, fn, arg) {
        var i, l;

        if (!stride) {
          stride = 2;
        }

        if (!offset) {
          offset = 0;
        }

        if (count) {
          l = Math.min(count * stride + offset, a.length);
        } else {
          l = a.length;
        }

        for (i = offset; i < l; i += stride) {
          vec[0] = a[i];
          vec[1] = a[i + 1];
          fn(vec, vec, arg);
          a[i] = vec[0];
          a[i + 1] = vec[1];
        }

        return a;
      };
    })();

    class Vector {
        x;
        y;
        z;
        constructor(x, y, z) {
            this.x = x;
            this.y = y;
            this.z = z;
        }
        length() {
            return Math.sqrt(this.x * this.x + this.y * this.y + this.z * this.z);
        }
        lengthSquared() {
            return this.x * this.x + this.y * this.y + this.z * this.z;
        }
        distance(other) {
            return this.sub(other).length();
        }
        distanceSquared(other) {
            return this.sub(other).lengthSquared();
        }
        abs() {
            return new Vector(Math.abs(this.x), Math.abs(this.y), Math.abs(this.z));
        }
        add(other) {
            return new Vector(this.x + other.x, this.y + other.y, this.z + other.z);
        }
        sub(other) {
            return new Vector(this.x - other.x, this.y - other.y, this.z - other.z);
        }
        mul(other) {
            return new Vector(this.x * other.x, this.y * other.y, this.z * other.z);
        }
        div(other) {
            return new Vector(this.x / other.x, this.y / other.y, this.z / other.z);
        }
        scale(n) {
            return new Vector(this.x * n, this.y * n, this.z * n);
        }
        dot(other) {
            return this.x * other.x + this.y * other.y + this.z * other.z;
        }
        cross(other) {
            const x = this.y * other.z - this.z * other.y;
            const y = this.z * other.x - this.x * other.z;
            const z = this.x * other.y - this.y * other.x;
            return new Vector(x, y, z);
        }
        normalize() {
            if (this.x == 0 && this.y == 0 && this.z == 0) {
                return this;
            }
            const r = 1 / this.length();
            return new Vector(this.x * r, this.y * r, this.z * r);
        }
        components() {
            return [this.x, this.y, this.z];
        }
        toString() {
            return `[${this.x} ${this.y} ${this.z}]`;
        }
    }

    var md5$1 = {exports: {}};

    var crypt = {exports: {}};

    (function() {
      var base64map
          = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/',

      crypt$1 = {
        // Bit-wise rotation left
        rotl: function(n, b) {
          return (n << b) | (n >>> (32 - b));
        },

        // Bit-wise rotation right
        rotr: function(n, b) {
          return (n << (32 - b)) | (n >>> b);
        },

        // Swap big-endian to little-endian and vice versa
        endian: function(n) {
          // If number given, swap endian
          if (n.constructor == Number) {
            return crypt$1.rotl(n, 8) & 0x00FF00FF | crypt$1.rotl(n, 24) & 0xFF00FF00;
          }

          // Else, assume array and swap all items
          for (var i = 0; i < n.length; i++)
            n[i] = crypt$1.endian(n[i]);
          return n;
        },

        // Generate an array of any length of random bytes
        randomBytes: function(n) {
          for (var bytes = []; n > 0; n--)
            bytes.push(Math.floor(Math.random() * 256));
          return bytes;
        },

        // Convert a byte array to big-endian 32-bit words
        bytesToWords: function(bytes) {
          for (var words = [], i = 0, b = 0; i < bytes.length; i++, b += 8)
            words[b >>> 5] |= bytes[i] << (24 - b % 32);
          return words;
        },

        // Convert big-endian 32-bit words to a byte array
        wordsToBytes: function(words) {
          for (var bytes = [], b = 0; b < words.length * 32; b += 8)
            bytes.push((words[b >>> 5] >>> (24 - b % 32)) & 0xFF);
          return bytes;
        },

        // Convert a byte array to a hex string
        bytesToHex: function(bytes) {
          for (var hex = [], i = 0; i < bytes.length; i++) {
            hex.push((bytes[i] >>> 4).toString(16));
            hex.push((bytes[i] & 0xF).toString(16));
          }
          return hex.join('');
        },

        // Convert a hex string to a byte array
        hexToBytes: function(hex) {
          for (var bytes = [], c = 0; c < hex.length; c += 2)
            bytes.push(parseInt(hex.substr(c, 2), 16));
          return bytes;
        },

        // Convert a byte array to a base-64 string
        bytesToBase64: function(bytes) {
          for (var base64 = [], i = 0; i < bytes.length; i += 3) {
            var triplet = (bytes[i] << 16) | (bytes[i + 1] << 8) | bytes[i + 2];
            for (var j = 0; j < 4; j++)
              if (i * 8 + j * 6 <= bytes.length * 8)
                base64.push(base64map.charAt((triplet >>> 6 * (3 - j)) & 0x3F));
              else
                base64.push('=');
          }
          return base64.join('');
        },

        // Convert a base-64 string to a byte array
        base64ToBytes: function(base64) {
          // Remove non-base-64 characters
          base64 = base64.replace(/[^A-Z0-9+\/]/ig, '');

          for (var bytes = [], i = 0, imod4 = 0; i < base64.length;
              imod4 = ++i % 4) {
            if (imod4 == 0) continue;
            bytes.push(((base64map.indexOf(base64.charAt(i - 1))
                & (Math.pow(2, -2 * imod4 + 8) - 1)) << (imod4 * 2))
                | (base64map.indexOf(base64.charAt(i)) >>> (6 - imod4 * 2)));
          }
          return bytes;
        }
      };

      crypt.exports = crypt$1;
    })();

    var charenc = {
      // UTF-8 encoding
      utf8: {
        // Convert a string to a byte array
        stringToBytes: function(str) {
          return charenc.bin.stringToBytes(unescape(encodeURIComponent(str)));
        },

        // Convert a byte array to a string
        bytesToString: function(bytes) {
          return decodeURIComponent(escape(charenc.bin.bytesToString(bytes)));
        }
      },

      // Binary encoding
      bin: {
        // Convert a string to a byte array
        stringToBytes: function(str) {
          for (var bytes = [], i = 0; i < str.length; i++)
            bytes.push(str.charCodeAt(i) & 0xFF);
          return bytes;
        },

        // Convert a byte array to a string
        bytesToString: function(bytes) {
          for (var str = [], i = 0; i < bytes.length; i++)
            str.push(String.fromCharCode(bytes[i]));
          return str.join('');
        }
      }
    };

    var charenc_1 = charenc;

    /*!
     * Determine if an object is a Buffer
     *
     * @author   Feross Aboukhadijeh <https://feross.org>
     * @license  MIT
     */

    // The _isBuffer check is for Safari 5-7 support, because it's missing
    // Object.prototype.constructor. Remove this eventually
    var isBuffer_1 = function (obj) {
      return obj != null && (isBuffer(obj) || isSlowBuffer(obj) || !!obj._isBuffer)
    };

    function isBuffer (obj) {
      return !!obj.constructor && typeof obj.constructor.isBuffer === 'function' && obj.constructor.isBuffer(obj)
    }

    // For Node v0.10 support. Remove this eventually.
    function isSlowBuffer (obj) {
      return typeof obj.readFloatLE === 'function' && typeof obj.slice === 'function' && isBuffer(obj.slice(0, 0))
    }

    (function(){
      var crypt$1 = crypt.exports,
          utf8 = charenc_1.utf8,
          isBuffer = isBuffer_1,
          bin = charenc_1.bin,

      // The core
      md5 = function (message, options) {
        // Convert to byte array
        if (message.constructor == String)
          if (options && options.encoding === 'binary')
            message = bin.stringToBytes(message);
          else
            message = utf8.stringToBytes(message);
        else if (isBuffer(message))
          message = Array.prototype.slice.call(message, 0);
        else if (!Array.isArray(message) && message.constructor !== Uint8Array)
          message = message.toString();
        // else, assume byte array already

        var m = crypt$1.bytesToWords(message),
            l = message.length * 8,
            a =  1732584193,
            b = -271733879,
            c = -1732584194,
            d =  271733878;

        // Swap endian
        for (var i = 0; i < m.length; i++) {
          m[i] = ((m[i] <<  8) | (m[i] >>> 24)) & 0x00FF00FF |
                 ((m[i] << 24) | (m[i] >>>  8)) & 0xFF00FF00;
        }

        // Padding
        m[l >>> 5] |= 0x80 << (l % 32);
        m[(((l + 64) >>> 9) << 4) + 14] = l;

        // Method shortcuts
        var FF = md5._ff,
            GG = md5._gg,
            HH = md5._hh,
            II = md5._ii;

        for (var i = 0; i < m.length; i += 16) {

          var aa = a,
              bb = b,
              cc = c,
              dd = d;

          a = FF(a, b, c, d, m[i+ 0],  7, -680876936);
          d = FF(d, a, b, c, m[i+ 1], 12, -389564586);
          c = FF(c, d, a, b, m[i+ 2], 17,  606105819);
          b = FF(b, c, d, a, m[i+ 3], 22, -1044525330);
          a = FF(a, b, c, d, m[i+ 4],  7, -176418897);
          d = FF(d, a, b, c, m[i+ 5], 12,  1200080426);
          c = FF(c, d, a, b, m[i+ 6], 17, -1473231341);
          b = FF(b, c, d, a, m[i+ 7], 22, -45705983);
          a = FF(a, b, c, d, m[i+ 8],  7,  1770035416);
          d = FF(d, a, b, c, m[i+ 9], 12, -1958414417);
          c = FF(c, d, a, b, m[i+10], 17, -42063);
          b = FF(b, c, d, a, m[i+11], 22, -1990404162);
          a = FF(a, b, c, d, m[i+12],  7,  1804603682);
          d = FF(d, a, b, c, m[i+13], 12, -40341101);
          c = FF(c, d, a, b, m[i+14], 17, -1502002290);
          b = FF(b, c, d, a, m[i+15], 22,  1236535329);

          a = GG(a, b, c, d, m[i+ 1],  5, -165796510);
          d = GG(d, a, b, c, m[i+ 6],  9, -1069501632);
          c = GG(c, d, a, b, m[i+11], 14,  643717713);
          b = GG(b, c, d, a, m[i+ 0], 20, -373897302);
          a = GG(a, b, c, d, m[i+ 5],  5, -701558691);
          d = GG(d, a, b, c, m[i+10],  9,  38016083);
          c = GG(c, d, a, b, m[i+15], 14, -660478335);
          b = GG(b, c, d, a, m[i+ 4], 20, -405537848);
          a = GG(a, b, c, d, m[i+ 9],  5,  568446438);
          d = GG(d, a, b, c, m[i+14],  9, -1019803690);
          c = GG(c, d, a, b, m[i+ 3], 14, -187363961);
          b = GG(b, c, d, a, m[i+ 8], 20,  1163531501);
          a = GG(a, b, c, d, m[i+13],  5, -1444681467);
          d = GG(d, a, b, c, m[i+ 2],  9, -51403784);
          c = GG(c, d, a, b, m[i+ 7], 14,  1735328473);
          b = GG(b, c, d, a, m[i+12], 20, -1926607734);

          a = HH(a, b, c, d, m[i+ 5],  4, -378558);
          d = HH(d, a, b, c, m[i+ 8], 11, -2022574463);
          c = HH(c, d, a, b, m[i+11], 16,  1839030562);
          b = HH(b, c, d, a, m[i+14], 23, -35309556);
          a = HH(a, b, c, d, m[i+ 1],  4, -1530992060);
          d = HH(d, a, b, c, m[i+ 4], 11,  1272893353);
          c = HH(c, d, a, b, m[i+ 7], 16, -155497632);
          b = HH(b, c, d, a, m[i+10], 23, -1094730640);
          a = HH(a, b, c, d, m[i+13],  4,  681279174);
          d = HH(d, a, b, c, m[i+ 0], 11, -358537222);
          c = HH(c, d, a, b, m[i+ 3], 16, -722521979);
          b = HH(b, c, d, a, m[i+ 6], 23,  76029189);
          a = HH(a, b, c, d, m[i+ 9],  4, -640364487);
          d = HH(d, a, b, c, m[i+12], 11, -421815835);
          c = HH(c, d, a, b, m[i+15], 16,  530742520);
          b = HH(b, c, d, a, m[i+ 2], 23, -995338651);

          a = II(a, b, c, d, m[i+ 0],  6, -198630844);
          d = II(d, a, b, c, m[i+ 7], 10,  1126891415);
          c = II(c, d, a, b, m[i+14], 15, -1416354905);
          b = II(b, c, d, a, m[i+ 5], 21, -57434055);
          a = II(a, b, c, d, m[i+12],  6,  1700485571);
          d = II(d, a, b, c, m[i+ 3], 10, -1894986606);
          c = II(c, d, a, b, m[i+10], 15, -1051523);
          b = II(b, c, d, a, m[i+ 1], 21, -2054922799);
          a = II(a, b, c, d, m[i+ 8],  6,  1873313359);
          d = II(d, a, b, c, m[i+15], 10, -30611744);
          c = II(c, d, a, b, m[i+ 6], 15, -1560198380);
          b = II(b, c, d, a, m[i+13], 21,  1309151649);
          a = II(a, b, c, d, m[i+ 4],  6, -145523070);
          d = II(d, a, b, c, m[i+11], 10, -1120210379);
          c = II(c, d, a, b, m[i+ 2], 15,  718787259);
          b = II(b, c, d, a, m[i+ 9], 21, -343485551);

          a = (a + aa) >>> 0;
          b = (b + bb) >>> 0;
          c = (c + cc) >>> 0;
          d = (d + dd) >>> 0;
        }

        return crypt$1.endian([a, b, c, d]);
      };

      // Auxiliary functions
      md5._ff  = function (a, b, c, d, x, s, t) {
        var n = a + (b & c | ~b & d) + (x >>> 0) + t;
        return ((n << s) | (n >>> (32 - s))) + b;
      };
      md5._gg  = function (a, b, c, d, x, s, t) {
        var n = a + (b & d | c & ~d) + (x >>> 0) + t;
        return ((n << s) | (n >>> (32 - s))) + b;
      };
      md5._hh  = function (a, b, c, d, x, s, t) {
        var n = a + (b ^ c ^ d) + (x >>> 0) + t;
        return ((n << s) | (n >>> (32 - s))) + b;
      };
      md5._ii  = function (a, b, c, d, x, s, t) {
        var n = a + (c ^ (b | ~d)) + (x >>> 0) + t;
        return ((n << s) | (n >>> (32 - s))) + b;
      };

      // Package private blocksize
      md5._blocksize = 16;
      md5._digestsize = 16;

      md5$1.exports = function (message, options) {
        if (message === undefined || message === null)
          throw new Error('Illegal argument ' + message);

        var digestbytes = crypt$1.wordsToBytes(md5(message, options));
        return options && options.asBytes ? digestbytes :
            options && options.asString ? bin.bytesToString(digestbytes) :
            crypt$1.bytesToHex(digestbytes);
      };

    })();

    var md5 = md5$1.exports;

    class LegacyRandom {
        static MODULUS_BITS = 48;
        static MODULUS_MASK = BigInt('281474976710655');
        static MULTIPLIER = BigInt('25214903917');
        static INCREMENT = BigInt('11');
        static FLOAT_MULTIPLIER = 1 / Math.pow(2, 24);
        static DOUBLE_MULTIPLIER = 1 / Math.pow(2, 30);
        seed = BigInt(0);
        constructor(seed) {
            this.setSeed(seed);
        }
        static fromLargeFeatureSeed(worldSeed, x, z) {
            const random = new LegacyRandom(worldSeed);
            const a = random.nextLong();
            const b = random.nextLong();
            const seed = BigInt(x) * a ^ BigInt(z) * b ^ worldSeed;
            random.setSeed(seed);
            return random;
        }
        static fromLargeFeatureWithSalt(worldSeed, x, z, salt) {
            const seed = BigInt(x) * BigInt('341873128712') + BigInt(z) * BigInt('132897987541') + worldSeed + BigInt(salt);
            return new LegacyRandom(seed);
        }
        fork() {
            return new LegacyRandom(this.nextLong());
        }
        forkPositional() {
            return new LegacyPositionalRandom(this.nextLong());
        }
        setSeed(seed) {
            this.seed = (seed ^ LegacyRandom.MULTIPLIER) & LegacyRandom.MODULUS_MASK;
        }
        advance() {
            this.seed = this.seed * LegacyRandom.MULTIPLIER + LegacyRandom.INCREMENT & LegacyRandom.MODULUS_MASK;
        }
        consume(count) {
            for (let i = 0; i < count; i += 1) {
                this.advance();
            }
        }
        next(bits) {
            this.advance();
            const out = Number(this.seed >> BigInt(LegacyRandom.MODULUS_BITS - bits));
            return out > 2147483647 ? out - 4294967296 : out;
        }
        nextInt(max) {
            if (max === undefined) {
                return this.next(32);
            }
            if ((max & max - 1) == 0) { // If max is a power of two
                return Number(BigInt(max) * BigInt(this.next(31)) >> BigInt(31));
            }
            let a, b;
            while ((a = this.next(31)) - (b = a % max) + (max - 1) < 0) { }
            return b;
        }
        nextLong() {
            return (BigInt(this.next(32)) << BigInt(32)) + BigInt(this.next(32));
        }
        nextFloat() {
            return this.next(24) * LegacyRandom.FLOAT_MULTIPLIER;
        }
        nextDouble() {
            const a = this.next(30);
            this.advance();
            return a * LegacyRandom.DOUBLE_MULTIPLIER;
        }
    }
    class LegacyPositionalRandom {
        seed;
        constructor(seed) {
            this.seed = seed;
        }
        at(x, y, z) {
            const seed = getSeed(x, y, z);
            return new LegacyRandom(seed ^ this.seed);
        }
        fromHashOf(name) {
            const hash = md5(name, { asBytes: true });
            const seed = longfromBytes(hash[0], hash[1], hash[2], hash[3], hash[4], hash[5], hash[6], hash[7]);
            return new LegacyRandom(seed ^ this.seed);
        }
        seedKey() {
            return [this.seed, BigInt(0)];
        }
    }

    class XoroshiroRandom {
        static SILVER_RATIO_64 = BigInt('7640891576956012809');
        static GOLDEN_RATIO_64 = BigInt('-7046029254386353131');
        static FLOAT_MULTIPLIER = 1 / Math.pow(2, 24);
        static DOUBLE_MULTIPLIER = 1.1102230246251565E-16;
        static BIGINT_1 = BigInt(1);
        static BIGINT_17 = BigInt(17);
        static BIGINT_21 = BigInt(21);
        static BIGINT_27 = BigInt(27);
        static BIGINT_28 = BigInt(28);
        static BIGINT_30 = BigInt(30);
        static BIGINT_31 = BigInt(31);
        static BIGINT_32 = BigInt(32);
        static BIGINT_49 = BigInt(49);
        static BIGINT_64 = BigInt(64);
        static STAFFORD_1 = BigInt('-4658895280553007687');
        static STAFFORD_2 = BigInt('-7723592293110705685');
        static MAX_ULONG = BigInt('0xFFFFFFFFFFFFFFFF');
        static POW2_60 = BigInt('0x10000000000000000');
        static POW2_63 = BigInt('0x8000000000000000');
        static MAX_UINT = BigInt(0xFFFFFFFF);
        seed = [BigInt(0), BigInt(0)];
        constructor(seed) {
            this.seed = seed;
        }
        static create(seed) {
            return new XoroshiroRandom(XoroshiroRandom.upgradeSeedTo128bit(seed));
        }
        static mixStafford13(value) {
            value = ((value ^ value >> XoroshiroRandom.BIGINT_30) * XoroshiroRandom.STAFFORD_1) & XoroshiroRandom.MAX_ULONG;
            value = ((value ^ value >> XoroshiroRandom.BIGINT_27) * XoroshiroRandom.STAFFORD_2) & XoroshiroRandom.MAX_ULONG;
            return (value ^ value >> XoroshiroRandom.BIGINT_31) & XoroshiroRandom.MAX_ULONG;
        }
        static upgradeSeedTo128bit(seed) {
            if (seed < 0) {
                seed += XoroshiroRandom.POW2_60;
            }
            const seedLo = seed ^ XoroshiroRandom.SILVER_RATIO_64;
            const seedHi = (seedLo + XoroshiroRandom.GOLDEN_RATIO_64) & XoroshiroRandom.MAX_ULONG;
            return [XoroshiroRandom.mixStafford13(seedLo), XoroshiroRandom.mixStafford13(seedHi)];
        }
        static rotateLeft(value, shift) {
            return (value << shift) & (XoroshiroRandom.MAX_ULONG) | (value >> (XoroshiroRandom.BIGINT_64 - shift));
        }
        setSeed(seed) {
            this.seed = XoroshiroRandom.upgradeSeedTo128bit(seed);
        }
        fork() {
            return new XoroshiroRandom([this.next(), this.next()]);
        }
        forkPositional() {
            return new XoroshiroPositionalRandom(this.next(), this.next());
        }
        next() {
            const seedLo = this.seed[0];
            let seedHi = this.seed[1];
            const value = (XoroshiroRandom.rotateLeft((seedLo + seedHi) & XoroshiroRandom.MAX_ULONG, XoroshiroRandom.BIGINT_17) + seedLo) & XoroshiroRandom.MAX_ULONG;
            seedHi ^= seedLo;
            this.seed = [
                XoroshiroRandom.rotateLeft(seedLo, XoroshiroRandom.BIGINT_49) ^ seedHi ^ ((seedHi << XoroshiroRandom.BIGINT_21) & XoroshiroRandom.MAX_ULONG),
                XoroshiroRandom.rotateLeft(seedHi, XoroshiroRandom.BIGINT_28),
            ];
            return value;
        }
        nextLong() {
            let value = this.next();
            if (value > XoroshiroRandom.POW2_63)
                value -= XoroshiroRandom.POW2_60;
            return value;
        }
        consume(count) {
            let seedLo = this.seed[0];
            let seedHi = this.seed[1];
            for (let i = 0; i < count; i += 1) {
                seedHi ^= seedLo;
                seedLo = XoroshiroRandom.rotateLeft(seedLo, XoroshiroRandom.BIGINT_49) ^ seedHi ^ seedHi << XoroshiroRandom.BIGINT_21;
                seedHi = XoroshiroRandom.rotateLeft(seedHi, XoroshiroRandom.BIGINT_28);
            }
            this.seed = [seedLo, seedHi];
        }
        nextBits(bits) {
            return this.next() >> (BigInt(64 - bits));
        }
        nextInt(max) {
            let value = this.next() & XoroshiroRandom.MAX_UINT;
            if (!max) {
                let result = Number(value);
                if (result >= 0x80000000) {
                    result -= 0x100000000;
                }
                return result;
            }
            else {
                const maxBigint = BigInt(max);
                let product = value * maxBigint;
                let productLo = product & XoroshiroRandom.MAX_UINT;
                if (productLo < maxBigint) {
                    const newMax = ((~maxBigint & XoroshiroRandom.MAX_UINT) + XoroshiroRandom.BIGINT_1) % maxBigint;
                    while (productLo < newMax) {
                        value = this.next() & XoroshiroRandom.MAX_UINT;
                        product = value * maxBigint;
                        productLo = product & XoroshiroRandom.MAX_UINT;
                    }
                }
                const productHi = product >> XoroshiroRandom.BIGINT_32;
                return Number(productHi);
            }
        }
        nextFloat() {
            return Number(this.nextBits(24)) * XoroshiroRandom.FLOAT_MULTIPLIER;
        }
        nextDouble() {
            return Number(this.nextBits(53)) * XoroshiroRandom.DOUBLE_MULTIPLIER;
        }
        parityConfigString() {
            return 'seedLo: ' + this.seed[0] + ', seedHi: ' + this.seed[1];
        }
    }
    class XoroshiroPositionalRandom {
        seedLo;
        seedHi;
        constructor(seedLo, seedHi) {
            this.seedLo = seedLo;
            this.seedHi = seedHi;
        }
        at(x, y, z) {
            const positionSeed = getSeed(x, y, z);
            const seedLo = positionSeed ^ this.seedLo;
            return new XoroshiroRandom([seedLo, this.seedHi]);
        }
        fromHashOf(name) {
            const hash = md5(name, { asBytes: true });
            const lo = longfromBytes(hash[0], hash[1], hash[2], hash[3], hash[4], hash[5], hash[6], hash[7]);
            const hi = longfromBytes(hash[8], hash[9], hash[10], hash[11], hash[12], hash[13], hash[14], hash[15]);
            return new XoroshiroRandom([lo ^ this.seedLo, hi ^ this.seedHi]);
        }
        seedKey() {
            return [this.seedLo, this.seedHi];
        }
    }

    class SimplexNoise {
        static GRADIENT = [[1, 1, 0], [-1, 1, 0], [1, -1, 0], [-1, -1, 0], [1, 0, 1], [-1, 0, 1], [1, 0, -1], [-1, 0, -1], [0, 1, 1], [0, -1, 1], [0, 1, -1], [0, -1, -1], [1, 1, 0], [0, -1, 1], [-1, 1, 0], [0, -1, -1]];
        static F2 = 0.5 * (Math.sqrt(3.0) - 1.0);
        static G2 = (3.0 - Math.sqrt(3.0)) / 6.0;
        p;
        xo;
        yo;
        zo;
        constructor(random) {
            this.xo = random.nextDouble() * 256;
            this.yo = random.nextDouble() * 256;
            this.zo = random.nextDouble() * 256;
            this.p = Array(256);
            for (let i = 0; i < 256; i += 1) {
                this.p[i] = i;
            }
            for (let i = 0; i < 256; i += 1) {
                const j = random.nextInt(256 - i);
                const b = this.p[i];
                this.p[i] = this.p[i + j];
                this.p[i + j] = b;
            }
        }
        sample2D(d, d2) {
            const d6 = (d + d2) * SimplexNoise.F2;
            const n4 = intFloor(d + d6);
            const n3 = intFloor(d2 + d6);
            const d3 = (n4 + n3) * SimplexNoise.G2;
            const d7 = n4 - d3;
            const d8 = d - d7;
            let a;
            let b;
            const d4 = d2 - (n3 - d3);
            if (d8 > d4) {
                a = 1;
                b = 0;
            }
            else {
                a = 0;
                b = 1;
            }
            const d9 = d8 - a + SimplexNoise.G2;
            const d10 = d4 - b + SimplexNoise.G2;
            const d11 = d8 - 1.0 + 2.0 * SimplexNoise.G2;
            const d12 = d4 - 1.0 + 2.0 * SimplexNoise.G2;
            const n5 = n4 & 0xFF;
            const n6 = n3 & 0xFF;
            const n7 = this.P(n5 + this.P(n6)) % 12;
            const n8 = this.P(n5 + a + this.P(n6 + b)) % 12;
            const n9 = this.P(n5 + 1 + this.P(n6 + 1)) % 12;
            const d13 = this.getCornerNoise3D(n7, d8, d4, 0.0, 0.5);
            const d14 = this.getCornerNoise3D(n8, d9, d10, 0.0, 0.5);
            const d15 = this.getCornerNoise3D(n9, d11, d12, 0.0, 0.5);
            return 70.0 * (d13 + d14 + d15);
        }
        sample(x, y, z) {
            const d5 = (x + y + z) * 0.3333333333333333;
            const x2 = intFloor(x + d5);
            const y2 = intFloor(y + d5);
            const z2 = intFloor(z + d5);
            const d7 = (x2 + y2 + z2) * 0.16666666666666666;
            const x3 = x - (x2 - d7);
            const y3 = y - (y2 - d7);
            const z3 = z - (z2 - d7);
            let a;
            let b;
            let c;
            let d;
            let e;
            let f;
            if (x3 >= y3) {
                if (y3 >= z3) {
                    a = 1;
                    b = 0;
                    c = 0;
                    d = 1;
                    e = 1;
                    f = 0;
                }
                else if (x3 >= z3) {
                    a = 1;
                    b = 0;
                    c = 0;
                    d = 1;
                    e = 0;
                    f = 1;
                }
                else {
                    a = 0;
                    b = 0;
                    c = 1;
                    d = 1;
                    e = 0;
                    f = 1;
                }
            }
            else if (y3 < z3) {
                a = 0;
                b = 0;
                c = 1;
                d = 0;
                e = 1;
                f = 1;
            }
            else if (x3 < z3) {
                a = 0;
                b = 1;
                c = 0;
                d = 0;
                e = 1;
                f = 1;
            }
            else {
                a = 0;
                b = 1;
                c = 0;
                d = 1;
                e = 1;
                f = 0;
            }
            const x4 = x3 - a + 0.16666666666666666;
            const y4 = y3 - b + 0.16666666666666666;
            const z4 = z3 - c + 0.16666666666666666;
            const x5 = x3 - d + 0.3333333333333333;
            const y5 = y3 - e + 0.3333333333333333;
            const z5 = z3 - f + 0.3333333333333333;
            const x6 = x3 - 0.5;
            const y6 = y3 - 0.5;
            const z6 = z3 - 0.5;
            const x7 = x2 & 0xFF;
            const y7 = y2 & 0xFF;
            const z7 = z2 & 0xFF;
            const g = this.P(x7 + this.P(y7 + this.P(z7))) % 12;
            const h = this.P(x7 + a + this.P(y7 + b + this.P(z7 + c))) % 12;
            const i = this.P(x7 + d + this.P(y7 + e + this.P(z7 + f))) % 12;
            const j = this.P(x7 + 1 + this.P(y7 + 1 + this.P(z7 + 1))) % 12;
            const k = this.getCornerNoise3D(g, x3, y3, z3, 0.6);
            const l = this.getCornerNoise3D(h, x4, y4, z4, 0.6);
            const m = this.getCornerNoise3D(i, x5, y5, z5, 0.6);
            const n = this.getCornerNoise3D(j, x6, y6, z6, 0.6);
            return 32.0 * (k + l + m + n);
        }
        P(i) {
            return this.p[i & 0xFF];
        }
        getCornerNoise3D(i, a, b, c, d) {
            let f;
            let e = d - a * a - b * b - c * c;
            if (e < 0.0) {
                f = 0.0;
            }
            else {
                e *= e;
                f = e * e * SimplexNoise.gradDot(i, a, b, c);
            }
            return f;
        }
        static gradDot(a, b, c, d) {
            const grad = SimplexNoise.GRADIENT[a & 15];
            return grad[0] * b + grad[1] * c + grad[2] * d;
        }
    }

    class ImprovedNoise {
        p;
        xo;
        yo;
        zo;
        constructor(random) {
            this.xo = random.nextDouble() * 256;
            this.yo = random.nextDouble() * 256;
            this.zo = random.nextDouble() * 256;
            this.p = Array(256);
            for (let i = 0; i < 256; i += 1) {
                this.p[i] = i > 127 ? i - 256 : i;
            }
            for (let i = 0; i < 256; i += 1) {
                const j = random.nextInt(256 - i);
                const b = this.p[i];
                this.p[i] = this.p[i + j];
                this.p[i + j] = b;
            }
        }
        sample(x, y, z, yScale = 0, yLimit = 0) {
            const x2 = x + this.xo;
            const y2 = y + this.yo;
            const z2 = z + this.zo;
            const x3 = intFloor(x2);
            const y3 = intFloor(y2);
            const z3 = intFloor(z2);
            const x4 = x2 - x3;
            const y4 = y2 - y3;
            const z4 = z2 - z3;
            let y6 = 0;
            if (yScale !== 0) {
                const t = yLimit >= 0 && yLimit < y4 ? yLimit : y4;
                y6 = intFloor(t / yScale + 1e-7) * yScale;
            }
            return this.sampleAndLerp(x3, y3, z3, x4, y4 - y6, z4, y4);
        }
        sampleAndLerp(a, b, c, d, e, f, g) {
            const h = this.P(a);
            const i = this.P(a + 1);
            const j = this.P(h + b);
            const k = this.P(h + b + 1);
            const l = this.P(i + b);
            const m = this.P(i + b + 1);
            const n = SimplexNoise.gradDot(this.P(j + c), d, e, f);
            const o = SimplexNoise.gradDot(this.P(l + c), d - 1.0, e, f);
            const p = SimplexNoise.gradDot(this.P(k + c), d, e - 1.0, f);
            const q = SimplexNoise.gradDot(this.P(m + c), d - 1.0, e - 1.0, f);
            const r = SimplexNoise.gradDot(this.P(j + c + 1), d, e, f - 1.0);
            const s = SimplexNoise.gradDot(this.P(l + c + 1), d - 1.0, e, f - 1.0);
            const t = SimplexNoise.gradDot(this.P(k + c + 1), d, e - 1.0, f - 1.0);
            const u = SimplexNoise.gradDot(this.P(m + c + 1), d - 1.0, e - 1.0, f - 1.0);
            const v = smoothstep(d);
            const w = smoothstep(g);
            const x = smoothstep(f);
            return lerp3(v, w, x, n, o, p, q, r, s, t, u);
        }
        P(i) {
            return this.p[i & 0xFF] & 0xFF;
        }
    }

    class PerlinNoise {
        noiseLevels;
        amplitudes;
        lowestFreqInputFactor;
        lowestFreqValueFactor;
        maxValue;
        constructor(random, firstOctave, amplitudes, forceLegacy = false) {
            if (random instanceof XoroshiroRandom && !forceLegacy) {
                const forkedRandom = random.forkPositional();
                this.noiseLevels = Array(amplitudes.length);
                for (let i = 0; i < amplitudes.length; i++) {
                    if (amplitudes[i] !== 0.0) {
                        const octave = firstOctave + i;
                        this.noiseLevels[i] = new ImprovedNoise(forkedRandom.fromHashOf('octave_' + octave));
                    }
                }
            }
            else {
                if (1 - firstOctave < amplitudes.length) {
                    throw new Error('Positive octaves are not allowed when using LegacyRandom');
                }
                this.noiseLevels = Array(amplitudes.length);
                for (let i = -firstOctave; i >= 0; i -= 1) {
                    if (i < amplitudes.length && amplitudes[i] !== 0) {
                        this.noiseLevels[i] = new ImprovedNoise(random);
                    }
                    else {
                        random.consume(262);
                    }
                }
            }
            this.amplitudes = amplitudes;
            this.lowestFreqInputFactor = Math.pow(2, firstOctave);
            this.lowestFreqValueFactor = Math.pow(2, (amplitudes.length - 1)) / (Math.pow(2, amplitudes.length) - 1);
            this.maxValue = this.edgeValue(2);
        }
        sample(x, y, z, yScale = 0, yLimit = 0, fixY = false) {
            let value = 0;
            let inputF = this.lowestFreqInputFactor;
            let valueF = this.lowestFreqValueFactor;
            for (let i = 0; i < this.noiseLevels.length; i += 1) {
                const noise = this.noiseLevels[i];
                if (noise) {
                    value += this.amplitudes[i] * valueF * noise.sample(PerlinNoise.wrap(x * inputF), fixY ? -noise.yo : PerlinNoise.wrap(y * inputF), PerlinNoise.wrap(z * inputF), yScale * inputF, yLimit * inputF);
                }
                inputF *= 2;
                valueF /= 2;
            }
            return value;
        }
        getOctaveNoise(i) {
            return this.noiseLevels[this.noiseLevels.length - 1 - i];
        }
        edgeValue(x) {
            let value = 0;
            let valueF = this.lowestFreqValueFactor;
            for (let i = 0; i < this.noiseLevels.length; i += 1) {
                if (this.noiseLevels[i]) {
                    value += this.amplitudes[i] * x * valueF;
                }
                valueF /= 2;
            }
            return value;
        }
        static wrap(value) {
            return value - longFloor(value / 3.3554432E7 + 0.5) * 3.3554432E7;
        }
    }

    class BlendedNoise {
        xzScale;
        yScale;
        xzFactor;
        yFactor;
        smearScaleMultiplier;
        minLimitNoise;
        maxLimitNoise;
        mainNoise;
        xzMultiplier;
        yMultiplier;
        maxValue;
        constructor(random, xzScale, yScale, xzFactor, yFactor, smearScaleMultiplier) {
            this.xzScale = xzScale;
            this.yScale = yScale;
            this.xzFactor = xzFactor;
            this.yFactor = yFactor;
            this.smearScaleMultiplier = smearScaleMultiplier;
            this.minLimitNoise = new PerlinNoise(random, -15, [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0], true);
            this.maxLimitNoise = new PerlinNoise(random, -15, [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0], true);
            this.mainNoise = new PerlinNoise(random, -7, [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0], true);
            this.xzMultiplier = 684.412 * xzScale;
            this.yMultiplier = 684.412 * yScale;
            this.maxValue = this.minLimitNoise.edgeValue(this.yMultiplier + 2);
        }
        sample(x, y, z) {
            const scaledX = x * this.xzMultiplier;
            const scaledY = y * this.yMultiplier;
            const scaledZ = z * this.xzMultiplier;
            const factoredX = scaledX / this.xzFactor;
            const factoredY = scaledY / this.yFactor;
            const factoredZ = scaledZ / this.xzFactor;
            const smear = this.yMultiplier * this.smearScaleMultiplier;
            const factoredSmear = smear / this.yFactor;
            let noise;
            let value = 0;
            let factor = 1;
            for (let i = 0; i < 8; i += 1) {
                noise = this.mainNoise.getOctaveNoise(i);
                if (noise) {
                    const xx = PerlinNoise.wrap(factoredX * factor);
                    const yy = PerlinNoise.wrap(factoredY * factor);
                    const zz = PerlinNoise.wrap(factoredZ * factor);
                    value += noise.sample(xx, yy, zz, factoredSmear * factor, factoredY * factor) / factor;
                }
                factor /= 2;
            }
            value = (value / 10 + 1) / 2;
            factor = 1;
            let min = 0;
            let max = 0;
            for (let i = 0; i < 16; i += 1) {
                const xx = PerlinNoise.wrap(scaledX * factor);
                const yy = PerlinNoise.wrap(scaledY * factor);
                const zz = PerlinNoise.wrap(scaledZ * factor);
                const smearsmear = smear * factor;
                if (value < 1 && (noise = this.minLimitNoise.getOctaveNoise(i))) {
                    min += noise.sample(xx, yy, zz, smearsmear, scaledY * factor) / factor;
                }
                if (value > 0 && (noise = this.maxLimitNoise.getOctaveNoise(i))) {
                    max += noise.sample(xx, yy, zz, smearsmear, scaledY * factor) / factor;
                }
                factor /= 2;
            }
            return clampedLerp(min / 512, max / 512, value) / 128;
        }
    }

    class NormalNoise {
        static INPUT_FACTOR = 1.0181268882175227;
        valueFactor;
        first;
        second;
        maxValue;
        constructor(random, { firstOctave, amplitudes }) {
            this.first = new PerlinNoise(random, firstOctave, amplitudes);
            this.second = new PerlinNoise(random, firstOctave, amplitudes);
            let min = +Infinity;
            let max = -Infinity;
            for (let i = 0; i < amplitudes.length; i += 1) {
                if (amplitudes[i] !== 0) {
                    min = Math.min(min, i);
                    max = Math.max(max, i);
                }
            }
            const expectedDeviation = 0.1 * (1 + 1 / (max - min + 1));
            this.valueFactor = (1 / 6) / expectedDeviation;
            this.maxValue = (this.first.maxValue + this.second.maxValue) * this.valueFactor;
        }
        sample(x, y, z) {
            const x2 = x * NormalNoise.INPUT_FACTOR;
            const y2 = y * NormalNoise.INPUT_FACTOR;
            const z2 = z * NormalNoise.INPUT_FACTOR;
            return (this.first.sample(x, y, z) + this.second.sample(x2, y2, z2)) * this.valueFactor;
        }
    }
    var NoiseParameters;
    (function (NoiseParameters) {
        function create(firstOctave, amplitudes) {
            return { firstOctave, amplitudes };
        }
        NoiseParameters.create = create;
        function fromJson(obj) {
            const root = Json.readObject(obj) ?? {};
            return {
                firstOctave: Json.readInt(root.firstOctave) ?? 0,
                amplitudes: Json.readArray(root.amplitudes, e => Json.readNumber(e) ?? 0) ?? [],
            };
        }
        NoiseParameters.fromJson = fromJson;
    })(NoiseParameters || (NoiseParameters = {}));

    const grass = [124 / 255, 189 / 255, 107 / 255];
    const spruce = Color.intToRgb(6396257);
    const birch = Color.intToRgb(8431445);
    const foliage = Color.intToRgb(4764952);
    const water = Color.intToRgb(4159204);
    const lily_pad = Color.intToRgb(2129968);
    const redstone = (power) => {
        const a = power / 15;
        const r = a * 0.6 + (a > 0 ? 0.4 : 0.3);
        const g = clamp$1(a * a * 0.7 - 0.5, 0, 1);
        const b = clamp$1(a * a * 0.6 - 0.7, 0, 1);
        return [r, g, b];
    };
    const stem = (age) => {
        return [age / 8, 1 - age / 32, age / 64];
    };
    const BlockColors = {
        large_fern: () => grass,
        tall_grass: () => grass,
        grass_block: () => grass,
        fern: () => grass,
        grass: () => grass,
        short_grass: () => grass,
        potted_fern: () => grass,
        pink_petals: () => grass,
        wildflowers: () => grass,
        bush: () => grass,
        spruce_leaves: () => spruce,
        birch_leaves: () => birch,
        oak_leaves: () => foliage,
        jungle_leaves: () => foliage,
        acacia_leaves: () => foliage,
        dark_oak_leaves: () => foliage,
        vine: () => foliage,
        mangrove_leaves: () => foliage,
        water: () => water,
        bubble_column: () => water,
        cauldron: () => water,
        water_cauldron: () => water,
        redstone_wire: (props) => redstone(parseInt(props['power'] ?? '0')),
        sugar_cane: () => grass,
        attached_melon_stem: () => stem(7),
        attached_pumpkin_stem: () => stem(7),
        melon_stem: (props) => stem(parseInt(props['age'] ?? '0')),
        pumpkin_stem: (props) => stem(parseInt(props['age'] ?? '0')),
        lily_pad: () => lily_pad,
    };

    var Cull;
    (function (Cull) {
        function rotate(cull, x, y) {
            let { up, down, north, east, south, west } = cull;
            switch (y) {
                case 90:
                    [north, east, south, west] = [east, south, west, north];
                    break;
                case 180:
                    [north, east, south, west] = [south, west, north, east];
                    break;
                case 270:
                    [north, east, south, west] = [west, north, east, south];
            }
            switch (x) {
                case 90:
                    [up, north, down, south] = [north, down, south, up];
                    break;
                case 180:
                    [up, north, down, south] = [down, south, up, north];
                    break;
                case 270:
                    [up, north, down, south] = [south, up, north, down];
            }
            return { up, down, north, east, south, west };
        }
        Cull.rotate = rotate;
        function none() {
            return Object.create(null);
        }
        Cull.none = none;
    })(Cull || (Cull = {}));

    class Vertex {
        pos;
        color;
        texture;
        textureLimit;
        normal;
        blockPos;
        static VEC = create$1();
        constructor(pos, color, texture, textureLimit, normal, blockPos) {
            this.pos = pos;
            this.color = color;
            this.texture = texture;
            this.textureLimit = textureLimit;
            this.normal = normal;
            this.blockPos = blockPos;
        }
        transform(transformation) {
            Vertex.VEC[0] = this.pos.x;
            Vertex.VEC[1] = this.pos.y;
            Vertex.VEC[2] = this.pos.z;
            transformMat4(Vertex.VEC, Vertex.VEC, transformation);
            this.pos = new Vector(Vertex.VEC[0], Vertex.VEC[1], Vertex.VEC[2]);
            return this;
        }
        static fromPos(pos) {
            return new Vertex(pos, [0, 0, 0], [0, 0], [0, 0, 0, 0], undefined, undefined);
        }
    }

    class Line {
        v1;
        v2;
        constructor(v1, v2) {
            this.v1 = v1;
            this.v2 = v2;
        }
        vertices() {
            return [this.v1, this.v2];
        }
        forEach(fn) {
            fn(this.v1);
            fn(this.v2);
            return this;
        }
        transform(transformation) {
            this.forEach(v => v.transform(transformation));
            return this;
        }
        setColor(color) {
            this.forEach(v => v.color = color);
            return this;
        }
        toString() {
            return `Line(${this.v1.pos.toString()}, ${this.v2.pos.toString()})`;
        }
        static fromPoints(p1, p2) {
            return new Line(Vertex.fromPos(p1), Vertex.fromPos(p2));
        }
    }

    class Mesh {
        quads;
        lines;
        posBuffer;
        colorBuffer;
        textureBuffer;
        textureLimitBuffer;
        normalBuffer;
        blockPosBuffer;
        indexBuffer;
        linePosBuffer;
        lineColorBuffer;
        constructor(quads = [], lines = []) {
            this.quads = quads;
            this.lines = lines;
        }
        clear() {
            this.quads = [];
            this.lines = [];
            return this;
        }
        isEmpty() {
            return this.quads.length === 0 && this.lines.length === 0;
        }
        quadVertices() {
            return this.quads.length * 4;
        }
        quadIndices() {
            return this.quads.length * 6;
        }
        lineVertices() {
            return this.lines.length * 2;
        }
        merge(other) {
            this.quads = this.quads.concat(other.quads);
            this.lines = this.lines.concat(other.lines);
            return this;
        }
        addLine(x1, y1, z1, x2, y2, z2, color) {
            const line = new Line(Vertex.fromPos(new Vector(x1, y1, z1)), Vertex.fromPos(new Vector(x2, y2, z2))).setColor(color);
            this.lines.push(line);
            return this;
        }
        addLineCube(x1, y1, z1, x2, y2, z2, color) {
            this.addLine(x1, y1, z1, x1, y1, z2, color);
            this.addLine(x2, y1, z1, x2, y1, z2, color);
            this.addLine(x1, y1, z1, x2, y1, z1, color);
            this.addLine(x1, y1, z2, x2, y1, z2, color);
            this.addLine(x1, y1, z1, x1, y2, z1, color);
            this.addLine(x2, y1, z1, x2, y2, z1, color);
            this.addLine(x1, y1, z2, x1, y2, z2, color);
            this.addLine(x2, y1, z2, x2, y2, z2, color);
            this.addLine(x1, y2, z1, x1, y2, z2, color);
            this.addLine(x2, y2, z1, x2, y2, z2, color);
            this.addLine(x1, y2, z1, x2, y2, z1, color);
            this.addLine(x1, y2, z2, x2, y2, z2, color);
            return this;
        }
        transform(transformation) {
            for (const quad of this.quads) {
                quad.transform(transformation);
            }
            return this;
        }
        computeNormals() {
            for (const quad of this.quads) {
                const normal = quad.normal();
                quad.forEach(v => v.normal = normal);
            }
        }
        rebuild(gl, options) {
            const rebuildBuffer = (buffer, type, data) => {
                if (!buffer) {
                    buffer = gl.createBuffer() ?? undefined;
                }
                if (!buffer) {
                    throw new Error('Cannot create new buffer');
                }
                gl.bindBuffer(type, buffer);
                gl.bufferData(type, data, gl.DYNAMIC_DRAW);
                return buffer;
            };
            const rebuildBufferV = (array, buffer, mapper) => {
                if (array.length === 0) {
                    if (buffer)
                        gl.deleteBuffer(buffer);
                    return undefined;
                }
                const data = array.flatMap(e => e.vertices().flatMap(v => {
                    const data = mapper(v);
                    if (!data)
                        throw new Error('Missing vertex component');
                    return data;
                }));
                return rebuildBuffer(buffer, gl.ARRAY_BUFFER, new Float32Array(data));
            };
            if (options.pos) {
                this.posBuffer = rebuildBufferV(this.quads, this.posBuffer, v => v.pos.components());
                this.linePosBuffer = rebuildBufferV(this.lines, this.linePosBuffer, v => v.pos.components());
            }
            if (options.color) {
                this.colorBuffer = rebuildBufferV(this.quads, this.colorBuffer, v => v.color);
                this.lineColorBuffer = rebuildBufferV(this.lines, this.lineColorBuffer, v => v.color);
            }
            if (options.texture) {
                this.textureBuffer = rebuildBufferV(this.quads, this.textureBuffer, v => v.texture);
                this.textureLimitBuffer = rebuildBufferV(this.quads, this.textureLimitBuffer, v => v.textureLimit);
            }
            if (options.normal) {
                this.normalBuffer = rebuildBufferV(this.quads, this.normalBuffer, v => v.normal?.components());
            }
            if (options.blockPos) {
                this.blockPosBuffer = rebuildBufferV(this.quads, this.blockPosBuffer, v => v.blockPos?.components());
            }
            if (this.quads.length === 0) {
                if (this.indexBuffer)
                    gl.deleteBuffer(this.indexBuffer);
                this.indexBuffer = undefined;
            }
            else {
                this.indexBuffer = rebuildBuffer(this.indexBuffer, gl.ELEMENT_ARRAY_BUFFER, new Uint16Array(this.quads.flatMap((_, i) => [4 * i, 4 * i + 1, 4 * i + 2, i * 4, 4 * i + 2, 4 * i + 3], true)));
            }
            return this;
        }
    }

    class BlockDefinition {
        variants;
        multipart;
        constructor(variants, multipart) {
            this.variants = variants;
            this.multipart = multipart;
        }
        getModelVariants(props) {
            if (this.variants) {
                const matches = Object.keys(this.variants).filter(v => this.matchesVariant(v, props));
                if (matches.length === 0)
                    return [];
                const variant = this.variants[matches[0]];
                return [Array.isArray(variant) ? variant[0] : variant];
            }
            else if (this.multipart) {
                const matches = this.multipart.filter(p => p.when ? this.matchesCase(p.when, props) : true);
                return matches.map(p => Array.isArray(p.apply) ? p.apply[0] : p.apply);
            }
            return [];
        }
        getMesh(name, props, atlas, blockModelProvider, cull) {
            const variants = this.getModelVariants(props);
            const mesh = new Mesh();
            for (const variant of variants) {
                const newCull = Cull.rotate(cull, variant.x ?? 0, variant.y ?? 0);
                const blockModel = blockModelProvider.getBlockModel(Identifier.parse(variant.model));
                if (!blockModel) {
                    throw new Error(`Cannot find block model ${variant.model}`);
                }
                const tint = name ? BlockColors[name.path]?.(props) : undefined;
                const variantMesh = blockModel.getMesh(atlas, newCull, tint);
                if (variant.x || variant.y) {
                    const t = create$2();
                    translate(t, t, [8, 8, 8]);
                    rotateY$1(t, t, -toRadian(variant.y ?? 0));
                    rotateX$1(t, t, -toRadian(variant.x ?? 0));
                    translate(t, t, [-8, -8, -8]);
                    variantMesh.transform(t);
                }
                mesh.merge(variantMesh);
            }
            const t = create$2();
            scale$1(t, t, [0.0625, 0.0625, 0.0625]);
            return mesh.transform(t);
        }
        matchesVariant(variant, props) {
            return variant.split(',').every(p => {
                const [k, v] = p.split('=');
                return props[k] === v;
            });
        }
        matchesCase(condition, props) {
            if (Array.isArray(condition.OR)) {
                return condition.OR.some(c => this.matchesCase(c, props));
            }
            if (Array.isArray(condition.AND)) {
                return condition.AND.every(c => this.matchesCase(c, props));
            }
            const states = condition;
            return Object.keys(states).every(k => {
                const values = states[k].split('|');
                return values.includes(props[k]);
            });
        }
        static fromJson(data) {
            return new BlockDefinition(data.variants, data.multipart);
        }
    }

    class Quad {
        v1;
        v2;
        v3;
        v4;
        constructor(v1, v2, v3, v4) {
            this.v1 = v1;
            this.v2 = v2;
            this.v3 = v3;
            this.v4 = v4;
        }
        vertices() {
            return [this.v1, this.v2, this.v3, this.v4];
        }
        forEach(fn) {
            fn(this.v1);
            fn(this.v2);
            fn(this.v3);
            fn(this.v4);
            return this;
        }
        transform(transformation) {
            this.forEach(v => v.transform(transformation));
            return this;
        }
        normal() {
            const e1 = this.v2.pos.sub(this.v1.pos);
            const e2 = this.v3.pos.sub(this.v1.pos);
            return e1.cross(e2).normalize();
        }
        reverse() {
            [this.v1, this.v2, this.v3, this.v4] = [this.v4, this.v3, this.v2, this.v1];
            return this;
        }
        setColor(color) {
            this.forEach(v => v.color = color);
            return this;
        }
        setTexture(texture, textureLimit) {
            this.v1.textureLimit = textureLimit;
            this.v2.textureLimit = textureLimit;
            this.v3.textureLimit = textureLimit;
            this.v4.textureLimit = textureLimit;
            this.v1.texture = [texture[0], texture[1]];
            this.v2.texture = [texture[2], texture[3]];
            this.v3.texture = [texture[4], texture[5]];
            this.v4.texture = [texture[6], texture[7]];
            return this;
        }
        toString() {
            return `Quad(${this.v1.pos.toString()}, ${this.v2.pos.toString()}, ${this.v3.pos.toString()}, ${this.v4.pos.toString()})`;
        }
        static fromPoints(p1, p2, p3, p4) {
            return new Quad(Vertex.fromPos(p1), Vertex.fromPos(p2), Vertex.fromPos(p3), Vertex.fromPos(p4));
        }
    }

    const faceRotations = {
        0: [0, 3, 2, 3, 2, 1, 0, 1],
        90: [2, 3, 2, 1, 0, 1, 0, 3],
        180: [2, 1, 0, 1, 0, 3, 2, 3],
        270: [0, 1, 0, 3, 2, 3, 2, 1],
    };
    const rotationAxis = {
        x: [1, 0, 0],
        y: [0, 1, 0],
        z: [0, 0, 1],
    };
    const SQRT2 = 1.41421356237;
    const rescaleAxis = {
        x: [1, SQRT2, SQRT2],
        y: [SQRT2, 1, SQRT2],
        z: [SQRT2, SQRT2, 1],
    };
    class BlockModel {
        parent;
        textures;
        elements;
        display;
        guiLight;
        static BUILTIN_GENERATED = Identifier.create('builtin/generated');
        static GENERATED_LAYERS = ['layer0', 'layer1', 'layer2', 'layer3', 'layer4'];
        generationMarker = false;
        constructor(parent, textures, elements, display, guiLight) {
            this.parent = parent;
            this.textures = textures;
            this.elements = elements;
            this.display = display;
            this.guiLight = guiLight;
        }
        getDisplayTransform(display) {
            const transform = this.display?.[display];
            const t = create$2();
            translate(t, t, [8, 8, 8]);
            if (transform?.translation) {
                translate(t, t, transform.translation);
            }
            if (transform?.rotation) {
                rotateX$1(t, t, transform.rotation[0] * Math.PI / 180);
                rotateY$1(t, t, transform.rotation[1] * Math.PI / 180);
                rotateZ(t, t, -transform.rotation[2] * Math.PI / 180);
            }
            if (transform?.scale) {
                scale$1(t, t, transform.scale);
            }
            translate(t, t, [-8, -8, -8]);
            return t;
        }
        getMesh(atlas, cull, tint) {
            const mesh = new Mesh();
            const getTint = (index) => {
                if (tint === undefined)
                    return [1, 1, 1];
                if (index === undefined || index < 0)
                    return [1, 1, 1];
                if (typeof tint === 'function')
                    return tint(index);
                return tint;
            };
            for (const e of this.elements ?? []) {
                mesh.merge(this.getElementMesh(e, atlas, cull, getTint));
            }
            return mesh;
        }
        getElementMesh(e, atlas, cull, getTint) {
            const mesh = new Mesh();
            const [x0, y0, z0] = e.from;
            const [x1, y1, z1] = e.to;
            const addFace = (face, uv, pos) => {
                const quad = Quad.fromPoints(new Vector(pos[0], pos[1], pos[2]), new Vector(pos[3], pos[4], pos[5]), new Vector(pos[6], pos[7], pos[8]), new Vector(pos[9], pos[10], pos[11]));
                const tint = getTint(face.tintindex);
                quad.setColor(tint);
                const [u0, v0, u1, v1] = atlas.getTextureUV(this.getTexture(face.texture));
                const du = (u1 - u0) / 16;
                const dv = (v1 - v0) / 16;
                uv[0] = (face.uv?.[0] ?? uv[0]) * du;
                uv[1] = (face.uv?.[1] ?? uv[1]) * dv;
                uv[2] = (face.uv?.[2] ?? uv[2]) * du;
                uv[3] = (face.uv?.[3] ?? uv[3]) * dv;
                const r = faceRotations[face.rotation ?? 0];
                quad.setTexture([
                    u0 + uv[r[0]], v0 + uv[r[1]],
                    u0 + uv[r[2]], v0 + uv[r[3]],
                    u0 + uv[r[4]], v0 + uv[r[5]],
                    u0 + uv[r[6]], v0 + uv[r[7]],
                ], [u0 + Math.min(uv[0], uv[2]), v0 + Math.min(uv[1], uv[3]), u0 + Math.max(uv[0], uv[2]), v0 + Math.max(uv[1], uv[3])]);
                mesh.quads.push(quad);
            };
            if (e.faces?.up?.texture && (!e.faces.up.cullface || !cull[e.faces.up.cullface])) {
                addFace(e.faces.up, [x0, 16 - z1, x1, 16 - z0], [x0, y1, z1, x1, y1, z1, x1, y1, z0, x0, y1, z0]);
            }
            if (e.faces?.down?.texture && (!e.faces.down.cullface || !cull[e.faces.down.cullface])) {
                addFace(e.faces.down, [16 - z1, 16 - x1, 16 - z0, 16 - x0], [x0, y0, z0, x1, y0, z0, x1, y0, z1, x0, y0, z1]);
            }
            if (e.faces?.south?.texture && (!e.faces.south.cullface || !cull[e.faces.south.cullface])) {
                addFace(e.faces.south, [x0, 16 - y1, x1, 16 - y0], [x0, y0, z1, x1, y0, z1, x1, y1, z1, x0, y1, z1]);
            }
            if (e.faces?.north?.texture && (!e.faces.north.cullface || !cull[e.faces.north.cullface])) {
                addFace(e.faces.north, [16 - x1, 16 - y1, 16 - x0, 16 - y0], [x1, y0, z0, x0, y0, z0, x0, y1, z0, x1, y1, z0]);
            }
            if (e.faces?.east?.texture && (!e.faces.east.cullface || !cull[e.faces.east.cullface])) {
                addFace(e.faces.east, [16 - z1, 16 - y1, 16 - z0, 16 - y0], [x1, y0, z1, x1, y0, z0, x1, y1, z0, x1, y1, z1]);
            }
            if (e.faces?.west?.texture && (!e.faces.west.cullface || !cull[e.faces.west.cullface])) {
                addFace(e.faces.west, [z0, 16 - y1, z1, 16 - y0], [x0, y0, z0, x0, y0, z1, x0, y1, z1, x0, y1, z0]);
            }
            const t = create$2();
            if (e.rotation) {
                const origin = fromValues$1(...e.rotation.origin);
                translate(t, t, origin);
                rotate(t, t, toRadian(e.rotation.angle), rotationAxis[e.rotation.axis]);
                if (e.rotation.rescale) {
                    scale$1(t, t, rescaleAxis[e.rotation.axis]);
                }
                negate(origin, origin);
                translate(t, t, origin);
            }
            return mesh.transform(t);
        }
        getTexture(textureRef) {
            let cur = textureRef;
            for (let depth = 0; depth < 32; depth++) {
                if (cur == null) {
                    return Identifier.parse('');
                }
                let s;
                if (typeof cur === 'string') {
                    s = cur;
                }
                else if (typeof cur === 'object' && typeof cur.sprite === 'string') {
                    s = cur.sprite;
                }
                else {
                    return Identifier.parse('');
                }
                if (s.startsWith('#')) {
                    cur = this.textures?.[s.slice(1)];
                    continue;
                }
                return Identifier.parse(s);
            }
            return Identifier.parse('');
        }
        flatten(accessor) {
            if (!this.parent) {
                return;
            }
            if (this.parent.equals(BlockModel.BUILTIN_GENERATED)) {
                this.generationMarker = true;
                return;
            }
            const parent = this.getParent(accessor);
            if (!parent) {
                console.warn(`parent ${this.parent} does not exist!`);
                this.parent = undefined;
                return;
            }
            parent.flatten(accessor);
            if (!this.elements) {
                this.elements = parent.elements;
            }
            if (!this.textures) {
                this.textures = {};
            }
            Object.keys(parent.textures ?? {}).forEach(t => {
                if (!this.textures[t]) {
                    this.textures[t] = parent.textures[t];
                }
            });
            if (!this.display) {
                this.display = {};
            }
            Object.keys(parent.display ?? {}).forEach(k => {
                const l = k;
                if (!this.display[l]) {
                    this.display[l] = parent.display[l];
                }
                else {
                    Object.keys(parent.display[l] ?? {}).forEach(m => {
                        const n = m;
                        if (!this.display[l][n]) {
                            this.display[l][n] = parent.display[l][n];
                        }
                    });
                }
            });
            if (!this.guiLight) {
                this.guiLight = parent.guiLight;
            }
            if (parent.generationMarker) {
                this.generationMarker = true;
            }
            if (this.generationMarker && (this.elements?.length ?? 0) === 0) {
                for (let i = 0; i < BlockModel.GENERATED_LAYERS.length; i += 1) {
                    const layer = BlockModel.GENERATED_LAYERS[i];
                    if (!Object.hasOwn(this.textures, layer)) {
                        break;
                    }
                    if (!this.elements) {
                        this.elements = [];
                    }
                    this.elements.push({
                        from: [0, 0, 0],
                        to: [16, 16, 0],
                        faces: { south: { texture: `#${layer}`, tintindex: i } },
                    });
                }
            }
            this.parent = undefined;
        }
        getParent(accessor) {
            if (!this.parent)
                return null;
            return accessor.getBlockModel(this.parent);
        }
        static fromJson(data) {
            const parent = data.parent === undefined ? undefined : Identifier.parse(data.parent);
            return new BlockModel(parent, data.textures, data.elements, data.display);
        }
    }

    function liquidRenderer(type, level, atlas, cull, tintindex) {
        const y = cull['up'] ? 16 : [14.2, 12.5, 10.5, 9, 7, 5.3, 3.7, 1.9, 16, 16, 16, 16, 16, 16, 16, 16][level];
        return new BlockModel(undefined, {
            still: `block/${type}_still`,
            flow: `block/${type}_flow`,
        }, [{
                from: [0, 0, 0],
                to: [16, y, 16],
                faces: {
                    up: { texture: '#still', tintindex, cullface: Direction.UP },
                    down: { texture: '#still', tintindex, cullface: Direction.DOWN },
                    north: { texture: '#flow', tintindex, cullface: Direction.NORTH },
                    east: { texture: '#flow', tintindex, cullface: Direction.EAST },
                    south: { texture: '#flow', tintindex, cullface: Direction.SOUTH },
                    west: { texture: '#flow', tintindex, cullface: Direction.WEST },
                },
            }]).getMesh(atlas, cull, BlockColors[type]?.({}));
    }
    const DyeColors = {
        white: Color.intToRgb(16383998),
        orange: Color.intToRgb(16351261),
        magenta: Color.intToRgb(13061821),
        light_blue: Color.intToRgb(3847130),
        yellow: Color.intToRgb(16701501),
        lime: Color.intToRgb(8439583),
        pink: Color.intToRgb(15961002),
        gray: Color.intToRgb(4673362),
        light_gray: Color.intToRgb(10329495),
        cyan: Color.intToRgb(1481884),
        purple: Color.intToRgb(8991416),
        blue: Color.intToRgb(3949738),
        brown: Color.intToRgb(8606770),
        green: Color.intToRgb(6192150),
        red: Color.intToRgb(11546150),
        black: Color.intToRgb(1908001),
    };
    var SpecialRenderers;
    (function (SpecialRenderers) {
        function chestRenderer(texture) {
            return (atlas) => {
                return new BlockModel(undefined, {
                    0: texture.withPrefix('entity/chest/').toString(),
                }, [
                    {
                        from: [1, 0, 1],
                        to: [15, 10, 15],
                        faces: {
                            north: { uv: [10.5, 8.25, 14, 10.75], rotation: 180, texture: '#0' },
                            east: { uv: [7, 8.25, 10.5, 10.75], rotation: 180, texture: '#0' },
                            south: { uv: [3.5, 8.25, 7, 10.75], rotation: 180, texture: '#0' },
                            west: { uv: [0, 8.25, 3.5, 10.75], rotation: 180, texture: '#0' },
                            up: { uv: [7, 4.75, 10.5, 8.25], texture: '#0' },
                            down: { uv: [3.5, 4.75, 7, 8.25], texture: '#0' },
                        },
                    },
                    {
                        from: [1, 10, 1],
                        to: [15, 14, 15],
                        faces: {
                            north: { uv: [10.5, 3.75, 14, 4.75], rotation: 180, texture: '#0' },
                            east: { uv: [7, 3.75, 10.5, 4.75], rotation: 180, texture: '#0' },
                            south: { uv: [3.5, 3.75, 7, 4.75], rotation: 180, texture: '#0' },
                            west: { uv: [0, 3.75, 3.5, 4.75], rotation: 180, texture: '#0' },
                            up: { uv: [7, 0, 10.5, 3.5], texture: '#0' },
                            down: { uv: [3.5, 0, 7, 3.5], texture: '#0' },
                        },
                    },
                    {
                        from: [7, 7, 0],
                        to: [9, 11, 2],
                        faces: {
                            north: { uv: [0.25, 0.25, 0.75, 1.25], rotation: 180, texture: '#0' },
                            east: { uv: [0, 0.25, 0.25, 1.25], rotation: 180, texture: '#0' },
                            south: { uv: [1, 0.25, 1.5, 1.25], rotation: 180, texture: '#0' },
                            west: { uv: [0.75, 0.25, 1, 1.25], rotation: 180, texture: '#0' },
                            up: { uv: [0.25, 0, 0.75, 0.25], rotation: 180, texture: '#0' },
                            down: { uv: [0.75, 0, 1.25, 0.25], rotation: 180, texture: '#0' },
                        },
                    },
                ]).getMesh(atlas, Cull.none());
            };
        }
        SpecialRenderers.chestRenderer = chestRenderer;
        function decoratedPotRenderer(atlas) {
            return new BlockModel(undefined, {
                0: 'entity/decorated_pot/decorated_pot_side',
                1: 'entity/decorated_pot/decorated_pot_base',
            }, [
                {
                    from: [1, 0, 1],
                    to: [15, 16, 15],
                    faces: {
                        north: { uv: [1, 0, 15, 16], texture: '#0' },
                        east: { uv: [1, 0, 15, 16], texture: '#0' },
                        south: { uv: [1, 0, 15, 16], texture: '#0' },
                        west: { uv: [1, 0, 15, 16], texture: '#0' },
                        up: { uv: [0, 6.5, 7, 13.5], texture: '#1' },
                        down: { uv: [7, 6.5, 14, 13.5], texture: '#1' },
                    },
                },
                {
                    from: [5, 16, 5],
                    to: [11, 17, 11],
                    faces: {
                        north: { uv: [0, 5.5, 3, 6], texture: '#1' },
                        east: { uv: [3, 5.5, 6, 6], texture: '#1' },
                        south: { uv: [6, 5.5, 9, 6], texture: '#1' },
                        west: { uv: [9, 5.5, 12, 6], texture: '#1' },
                    },
                },
                {
                    from: [4, 17, 4],
                    to: [12, 20, 12],
                    faces: {
                        north: { uv: [0, 4, 4, 5.5], texture: '#1' },
                        east: { uv: [4, 4, 8, 5.5], texture: '#1' },
                        south: { uv: [8, 4, 12, 5.5], texture: '#1' },
                        west: { uv: [12, 4, 16, 5.5], texture: '#1' },
                        up: { uv: [4, 0, 8, 4], texture: '#1' },
                        down: { uv: [8, 0, 12, 4], texture: '#1' },
                    },
                },
            ]).getMesh(atlas, Cull.none());
        }
        SpecialRenderers.decoratedPotRenderer = decoratedPotRenderer;
        function shieldRenderer(atlas) {
            return new BlockModel(undefined, {
                0: 'entity/shield_base_nopattern',
            }, [
                {
                    from: [-6, -11, -2],
                    to: [6, 11, -1],
                    faces: {
                        north: { uv: [3.5, 0.25, 6.5, 5.75], texture: '#0' },
                        east: { uv: [3.25, 0.25, 3.5, 5.75], texture: '#0' },
                        south: { uv: [0.25, 0.25, 3.25, 5.75], texture: '#0' },
                        west: { uv: [0, 0.25, 0.25, 5.75], texture: '#0' },
                        up: { uv: [0.25, 0, 3.25, 0.25], texture: '#0' },
                        down: { uv: [3.25, 0, 6.25, 0.25], texture: '#0' },
                    },
                },
            ]).getMesh(atlas, Cull.none());
        }
        SpecialRenderers.shieldRenderer = shieldRenderer;
        function headRenderer(texture, n) {
            return (atlas) => {
                return new BlockModel(undefined, {
                    0: texture.withPrefix('entity/').toString(),
                }, [
                    {
                        from: [4, 0, 4],
                        to: [12, 8, 12],
                        faces: {
                            north: { uv: [6, 2 * n, 8, 4 * n], texture: '#0' },
                            east: { uv: [2, 2 * n, 0, 4 * n], texture: '#0' },
                            south: { uv: [2, 2 * n, 4, 4 * n], texture: '#0' },
                            west: { uv: [6, 2 * n, 4, 4 * n], texture: '#0' },
                            up: { uv: [2, 0 * n, 4, 2 * n], texture: '#0' },
                            down: { uv: [4, 0 * n, 6, 2 * n], texture: '#0' },
                        },
                    },
                ])
                    .getMesh(atlas, Cull.none());
            };
        }
        SpecialRenderers.headRenderer = headRenderer;
        function dragonHeadRenderer(texture = Identifier.create('enderdragon/dragon')) {
            return (atlas) => {
                const transformation = create$2();
                translate(transformation, transformation, [8, 8, 8]);
                scale$1(transformation, transformation, [0.75, 0.75, 0.75]);
                rotateY$1(transformation, transformation, Math.PI);
                translate(transformation, transformation, [-8, -11.2, -8]);
                return new BlockModel(undefined, {
                    0: texture.withPrefix('entity/').toString(),
                }, [
                    {
                        from: [2, 4, -16],
                        to: [14, 9, 0],
                        faces: {
                            north: { uv: [12, 3.75, 12.75, 4.0625], texture: '#0' },
                            east: { uv: [11, 3.75, 12, 4.0625], texture: '#0' },
                            south: { uv: [13.75, 3.75, 14.5, 4.0625], texture: '#0' },
                            west: { uv: [12.75, 3.75, 13.75, 4.0625], texture: '#0' },
                            up: { uv: [12.75, 3.75, 12, 2.75], texture: '#0' },
                            down: { uv: [13.5, 2.75, 12.75, 3.75], texture: '#0' },
                        },
                    },
                    {
                        from: [0, 0, -2],
                        to: [16, 16, 14],
                        faces: {
                            north: { uv: [8, 2.875, 9, 3.875], texture: '#0' },
                            east: { uv: [7, 2.875, 8, 3.875], texture: '#0' },
                            south: { uv: [10, 2.875, 11, 3.875], texture: '#0' },
                            west: { uv: [9, 2.875, 10, 3.875], texture: '#0' },
                            up: { uv: [9, 2.875, 8, 1.875], texture: '#0' },
                            down: { uv: [10, 1.875, 9, 2.875], texture: '#0' },
                        },
                    },
                    {
                        from: [2, 0, -16],
                        to: [14, 4, 0],
                        rotation: { angle: -0.2 * 180 / Math.PI, axis: 'x', origin: [8, 4, -2] },
                        faces: {
                            north: { uv: [12, 5.0625, 12.75, 5.3125], texture: '#0' },
                            east: { uv: [11, 5.0625, 12, 5.3125], texture: '#0' },
                            south: { uv: [13.75, 5.0625, 14.5, 5.3125], texture: '#0' },
                            west: { uv: [12.75, 5.0625, 13.75, 5.3125], texture: '#0' },
                            up: { uv: [12.75, 5.0625, 12, 4.0625], texture: '#0' },
                            down: { uv: [13.5, 4.0625, 12.75, 5.0625], texture: '#0' },
                        },
                    },
                    {
                        from: [3, 16, 4],
                        to: [5, 20, 10],
                        faces: {
                            north: { uv: [0.375, 0.375, 0.5, 0.625], texture: '#0' },
                            east: { uv: [0, 0.375, 0.375, 0.625], texture: '#0' },
                            south: { uv: [0.875, 0.375, 1, 0.625], texture: '#0' },
                            west: { uv: [0.5, 0.375, 0.875, 0.625], texture: '#0' },
                            up: { uv: [0.5, 0.375, 0.375, 0], texture: '#0' },
                            down: { uv: [0.625, 0, 0.5, 0.375], texture: '#0' },
                        },
                    },
                    {
                        from: [11, 16, 4],
                        to: [13, 20, 10],
                        faces: {
                            north: { uv: [0.375, 0.375, 0.5, 0.625], texture: '#0' },
                            east: { uv: [0, 0.375, 0.375, 0.625], texture: '#0' },
                            south: { uv: [0.875, 0.375, 1, 0.625], texture: '#0' },
                            west: { uv: [0.5, 0.375, 0.875, 0.625], texture: '#0' },
                            up: { uv: [0.5, 0.375, 0.375, 0], texture: '#0' },
                            down: { uv: [0.625, 0, 0.5, 0.375], texture: '#0' },
                        },
                    },
                    {
                        from: [3, 9, -14],
                        to: [5, 11, -10],
                        faces: {
                            north: { uv: [7.25, 0.25, 7.375, 0.375], texture: '#0' },
                            east: { uv: [7, 0.25, 7.25, 0.375], texture: '#0' },
                            south: { uv: [7.625, 0.25, 7.75, 0.375], texture: '#0' },
                            west: { uv: [7.375, 0.25, 7.625, 0.375], texture: '#0' },
                            up: { uv: [7.375, 0.25, 7.25, 0], texture: '#0' },
                            down: { uv: [7.5, 0, 7.375, 0.25], texture: '#0' },
                        },
                    },
                    {
                        from: [11, 9, -14],
                        to: [13, 11, -10],
                        faces: {
                            north: { uv: [7.25, 0.25, 7.375, 0.375], texture: '#0' },
                            east: { uv: [7, 0.25, 7.25, 0.375], texture: '#0' },
                            south: { uv: [7.625, 0.25, 7.75, 0.375], texture: '#0' },
                            west: { uv: [7.375, 0.25, 7.625, 0.375], texture: '#0' },
                            up: { uv: [7.375, 0.25, 7.25, 0], texture: '#0' },
                            down: { uv: [7.5, 0, 7.375, 0.25], texture: '#0' },
                        },
                    },
                ]).getMesh(atlas, Cull.none()).transform(transformation);
            };
        }
        SpecialRenderers.dragonHeadRenderer = dragonHeadRenderer;
        function piglinHeadRenderer(texture = Identifier.create('piglin/piglin')) {
            return (atlas) => {
                return new BlockModel(undefined, {
                    0: texture.withPrefix('entity/').toString(),
                }, [
                    {
                        from: [3, 0, 4],
                        to: [13, 8, 12],
                        faces: {
                            north: { uv: [6.5, 2, 9, 4], texture: '#0' },
                            east: { uv: [2, 2, 0, 4], texture: '#0' },
                            south: { uv: [2, 2, 4.5, 4], texture: '#0' },
                            west: { uv: [6.5, 2, 4.5, 4], texture: '#0' },
                            up: { uv: [2, 0, 4.5, 2], texture: '#0' },
                            down: { uv: [4.5, 0, 7, 2], texture: '#0' },
                        },
                    },
                    {
                        from: [6, 0, 12],
                        to: [10, 4, 13],
                        faces: {
                            north: { uv: [9.25, 0.5, 10.25, 1.5], texture: '#0' },
                            east: { uv: [7.75, 0.5, 8, 1.5], texture: '#0' },
                            south: { uv: [8, 0.5, 9, 1.5], texture: '#0' },
                            west: { uv: [9, 0.5, 9.25, 1.5], texture: '#0' },
                            up: { uv: [8, 0.25, 9, 0.5], texture: '#0' },
                            down: { uv: [9, 0.25, 10, 0.5], texture: '#0' },
                        },
                    },
                    {
                        from: [5, 0, 12],
                        to: [6, 2, 13],
                        faces: {
                            north: { uv: [1.25, 0.25, 1.5, 0.75], texture: '#0' },
                            east: { uv: [0.5, 0.25, 0.75, 0.75], texture: '#0' },
                            south: { uv: [0.75, 0.25, 1, 0.75], texture: '#0' },
                            west: { uv: [1, 0.25, 1.25, 0.75], texture: '#0' },
                            up: { uv: [0.75, 0, 1, 0.25], texture: '#0' },
                            down: { uv: [1, 0, 1.25, 0.25], texture: '#0' },
                        },
                    },
                    {
                        from: [10, 0, 12],
                        to: [11, 2, 13],
                        faces: {
                            north: { uv: [1.25, 1.25, 1.5, 1.75], texture: '#0' },
                            east: { uv: [0.5, 1.25, 0.75, 1.75], texture: '#0' },
                            south: { uv: [0.75, 1.25, 1, 1.75], texture: '#0' },
                            west: { uv: [1, 1.25, 1.25, 1.75], texture: '#0' },
                            up: { uv: [0.75, 1, 1, 1.25], texture: '#0' },
                            down: { uv: [1, 1, 1.25, 1.25], texture: '#0' },
                        },
                    },
                    {
                        from: [2.5, 1.5, 6],
                        to: [3.5, 6.5, 10],
                        rotation: { angle: -30, axis: 'z', origin: [3, 7, 8] },
                        faces: {
                            north: { uv: [12, 2.5, 12.25, 3.75], texture: '#0' },
                            east: { uv: [9.75, 2.5, 10.75, 3.75], texture: '#0' },
                            south: { uv: [10.75, 2.5, 11, 3.75], texture: '#0' },
                            west: { uv: [11, 2.5, 12, 3.75], texture: '#0' },
                            up: { uv: [10.75, 1.5, 11, 2.5], texture: '#0' },
                            down: { uv: [11, 1.5, 11.25, 2.5], texture: '#0' },
                        },
                    },
                    {
                        from: [12.5, 1.5, 6],
                        to: [13.5, 6.5, 10],
                        rotation: { angle: 30, axis: 'z', origin: [13, 7, 8] },
                        faces: {
                            north: { uv: [15.25, 2.5, 15, 3.75], texture: '#0' },
                            east: { uv: [15, 2.5, 14, 3.75], texture: '#0' },
                            south: { uv: [14, 2.5, 13.75, 3.75], texture: '#0' },
                            west: { uv: [13.75, 2.5, 12.75, 3.75], texture: '#0' },
                            up: { uv: [14, 1.5, 13.75, 2.5], texture: '#0' },
                            down: { uv: [14.25, 1.5, 14, 2.5], texture: '#0' },
                        },
                    },
                ]).getMesh(atlas, Cull.none());
            };
        }
        SpecialRenderers.piglinHeadRenderer = piglinHeadRenderer;
        function signRenderer(texture) {
            return (atlas) => {
                return new BlockModel(undefined, {
                    0: texture.withPrefix('entity/signs/').toString(),
                }, [
                    {
                        from: [-4, 8, 7],
                        to: [20, 20, 9],
                        faces: {
                            north: { uv: [0.5, 1, 6.5, 7], texture: '#0' },
                            east: { uv: [0, 1, 0.5, 7], texture: '#0' },
                            south: { uv: [7, 1, 13, 7], texture: '#0' },
                            west: { uv: [6.5, 1, 7, 7], texture: '#0' },
                            up: { uv: [6.5, 1, 0.5, 0], texture: '#0' },
                            down: { uv: [12.5, 0, 6.5, 1], texture: '#0' },
                        },
                    },
                    {
                        from: [7, -6, 7],
                        to: [9, 8, 9],
                        faces: {
                            north: { uv: [0.5, 8, 1, 15], texture: '#0' },
                            east: { uv: [0, 8, 0.5, 15], texture: '#0' },
                            south: { uv: [1.5, 8, 2, 15], texture: '#0' },
                            west: { uv: [1, 8, 1.5, 15], texture: '#0' },
                            up: { uv: [1, 8, 0.5, 7], texture: '#0' },
                            down: { uv: [1.5, 7, 1, 8], texture: '#0' },
                        },
                    },
                ]).getMesh(atlas, Cull.none());
            };
        }
        SpecialRenderers.signRenderer = signRenderer;
        function wallSignRenderer(texture) {
            return (atlas) => {
                return new BlockModel(undefined, {
                    0: texture.withPrefix('entity/signs/').toString(),
                }, [
                    {
                        from: [-4, 4, 17],
                        to: [20, 16, 19],
                        faces: {
                            north: { uv: [0.5, 1, 6.5, 7], texture: '#0' },
                            east: { uv: [0, 1, 0.5, 7], texture: '#0' },
                            south: { uv: [7, 1, 13, 7], texture: '#0' },
                            west: { uv: [6.5, 1, 7, 7], texture: '#0' },
                            up: { uv: [6.5, 1, 0.5, 0], texture: '#0' },
                            down: { uv: [12.5, 0, 6.5, 1], texture: '#0' },
                        },
                    },
                ]).getMesh(atlas, Cull.none());
            };
        }
        SpecialRenderers.wallSignRenderer = wallSignRenderer;
        function hangingSignRenderer(texture) {
            return (attached, atlas) => {
                if (attached) {
                    return new BlockModel(undefined, {
                        0: texture.withPrefix('entity/signs/hanging/').toString(),
                    }, [
                        {
                            from: [1, 0, 7],
                            to: [15, 10, 9],
                            faces: {
                                north: { uv: [0.5, 7, 4, 12], texture: '#0' },
                                east: { uv: [0, 7, 0.5, 12], texture: '#0' },
                                south: { uv: [4.5, 7, 8, 12], texture: '#0' },
                                west: { uv: [4, 7, 4.5, 12], texture: '#0' },
                                up: { uv: [4, 7, 0.5, 6], texture: '#0' },
                                down: { uv: [7.5, 6, 4, 7], texture: '#0' },
                            },
                        },
                        {
                            from: [2, 10, 8],
                            to: [14, 16, 8],
                            faces: {
                                north: { uv: [3.5, 3, 6.5, 6], texture: '#0' },
                                south: { uv: [3.5, 3, 6.5, 6], texture: '#0' },
                            },
                        },
                    ]).getMesh(atlas, Cull.none());
                }
                return new BlockModel(undefined, {
                    0: texture.withPrefix('entity/signs/hanging/').toString(),
                }, [
                    {
                        from: [1, 0, 7],
                        to: [15, 10, 9],
                        faces: {
                            north: { uv: [0.5, 7, 4, 12], texture: '#0' },
                            east: { uv: [0, 7, 0.5, 12], texture: '#0' },
                            south: { uv: [4.5, 7, 8, 12], texture: '#0' },
                            west: { uv: [4, 7, 4.5, 12], texture: '#0' },
                            up: { uv: [4, 7, 0.5, 6], texture: '#0' },
                            down: { uv: [7.5, 6, 4, 7], texture: '#0' },
                        },
                    },
                    {
                        from: [1.5, 10, 8],
                        to: [4.5, 16, 8],
                        rotation: { angle: 45, axis: 'y', origin: [3, 12, 8] },
                        faces: {
                            north: { uv: [0, 3, 0.75, 6], texture: '#0' },
                            south: { uv: [0, 3, 0.75, 6], texture: '#0' },
                        },
                    },
                    {
                        from: [3, 10, 6.5],
                        to: [3, 16, 9.5],
                        rotation: { angle: 45, axis: 'y', origin: [3, 12, 8] },
                        faces: {
                            east: { uv: [1.5, 3, 2.25, 6], texture: '#0' },
                            west: { uv: [1.5, 3, 2.25, 6], texture: '#0' },
                        },
                    },
                    {
                        from: [11.5, 10, 8],
                        to: [14.5, 16, 8],
                        rotation: { angle: 45, axis: 'y', origin: [13, 12, 8] },
                        faces: {
                            north: { uv: [0, 3, 0.75, 6], texture: '#0' },
                            south: { uv: [0, 3, 0.75, 6], texture: '#0' },
                        },
                    },
                    {
                        from: [13, 10, 6.5],
                        to: [13, 16, 9.5],
                        rotation: { angle: 45, axis: 'y', origin: [13, 12, 8] },
                        faces: {
                            east: { uv: [1.5, 3, 2.25, 6], texture: '#0' },
                            west: { uv: [1.5, 3, 2.25, 6], texture: '#0' },
                        },
                    },
                ]).getMesh(atlas, Cull.none());
            };
        }
        SpecialRenderers.hangingSignRenderer = hangingSignRenderer;
        function wallHangingSignRenderer(woodType) {
            return (atlas) => {
                return new BlockModel(undefined, {
                    0: `entity/signs/hanging/${woodType}`,
                }, [
                    {
                        from: [1, 0, 7],
                        to: [15, 10, 9],
                        faces: {
                            north: { uv: [0.5, 7, 4, 12], texture: '#0' },
                            east: { uv: [0, 7, 0.5, 12], texture: '#0' },
                            south: { uv: [4.5, 7, 8, 12], texture: '#0' },
                            west: { uv: [4, 7, 4.5, 12], texture: '#0' },
                            up: { uv: [4, 7, 0.5, 6], texture: '#0' },
                            down: { uv: [7.5, 6, 4, 7], texture: '#0' },
                        },
                    },
                    {
                        from: [0, 14, 6],
                        to: [16, 16, 10],
                        faces: {
                            north: { uv: [1, 2, 5, 3], texture: '#0' },
                            east: { uv: [0, 2, 1, 3], texture: '#0' },
                            south: { uv: [6, 2, 10, 3], texture: '#0' },
                            west: { uv: [5, 2, 6, 3], texture: '#0' },
                            up: { uv: [5, 2, 1, 0], texture: '#0' },
                            down: { uv: [9, 0, 5, 2], texture: '#0' },
                        },
                    },
                    {
                        from: [1.5, 10, 8],
                        to: [4.5, 16, 8],
                        rotation: { angle: 45, axis: 'y', origin: [3, 12, 8] },
                        faces: {
                            north: { uv: [0, 3, 0.75, 6], texture: '#0' },
                            south: { uv: [0, 3, 0.75, 6], texture: '#0' },
                        },
                    },
                    {
                        from: [3, 10, 6.5],
                        to: [3, 16, 9.5],
                        rotation: { angle: 45, axis: 'y', origin: [3, 12, 8] },
                        faces: {
                            east: { uv: [1.5, 3, 2.25, 6], texture: '#0' },
                            west: { uv: [1.5, 3, 2.25, 6], texture: '#0' },
                        },
                    },
                    {
                        from: [11.5, 10, 8],
                        to: [14.5, 16, 8],
                        rotation: { angle: 45, axis: 'y', origin: [13, 12, 8] },
                        faces: {
                            north: { uv: [0, 3, 0.75, 6], texture: '#0' },
                            south: { uv: [0, 3, 0.75, 6], texture: '#0' },
                        },
                    },
                    {
                        from: [13, 10, 6.5],
                        to: [13, 16, 9.5],
                        rotation: { angle: 45, axis: 'y', origin: [13, 12, 8] },
                        faces: {
                            east: { uv: [1.5, 3, 2.25, 6], texture: '#0' },
                            west: { uv: [1.5, 3, 2.25, 6], texture: '#0' },
                        },
                    },
                ]).getMesh(atlas, Cull.none());
            };
        }
        SpecialRenderers.wallHangingSignRenderer = wallHangingSignRenderer;
        function conduitRenderer(atlas) {
            return new BlockModel(undefined, {
                0: 'entity/conduit/base',
            }, [
                {
                    from: [5, 5, 5],
                    to: [11, 11, 11],
                    faces: {
                        north: { uv: [3, 6, 6, 12], texture: '#0' },
                        east: { uv: [0, 6, 3, 12], texture: '#0' },
                        south: { uv: [9, 6, 12, 12], texture: '#0' },
                        west: { uv: [6, 6, 9, 12], texture: '#0' },
                        up: { uv: [6, 6, 3, 0], texture: '#0' },
                        down: { uv: [9, 0, 6, 6], texture: '#0' },
                    },
                },
            ]).getMesh(atlas, Cull.none());
        }
        SpecialRenderers.conduitRenderer = conduitRenderer;
        function shulkerBoxRenderer(texture) {
            return (atlas) => {
                return new BlockModel(undefined, {
                    0: texture.withPrefix('entity/shulker/').toString(),
                }, [
                    {
                        from: [0, 0, 0],
                        to: [16, 8, 16],
                        faces: {
                            north: { uv: [4, 11, 8, 13], texture: '#0' },
                            east: { uv: [0, 11, 4, 13], texture: '#0' },
                            south: { uv: [12, 11, 16, 13], texture: '#0' },
                            west: { uv: [8, 11, 12, 13], texture: '#0' },
                            up: { uv: [8, 11, 4, 7], texture: '#0' },
                            down: { uv: [12, 7, 8, 11], texture: '#0' },
                        },
                    },
                    {
                        from: [0, 4, 0],
                        to: [16, 16, 16],
                        faces: {
                            north: { uv: [4, 4, 8, 7], texture: '#0' },
                            east: { uv: [0, 4, 4, 7], texture: '#0' },
                            south: { uv: [12, 4, 16, 7], texture: '#0' },
                            west: { uv: [8, 4, 12, 7], texture: '#0' },
                            up: { uv: [8, 4, 4, 0], texture: '#0' },
                            down: { uv: [12, 0, 8, 4], texture: '#0' },
                        },
                    },
                ]).getMesh(atlas, Cull.none());
            };
        }
        SpecialRenderers.shulkerBoxRenderer = shulkerBoxRenderer;
        const bannerFace = (index) => ({
            north: { uv: [0.25, 0.25, 5.25, 10.25], texture: `#${index}`, tintindex: index },
            east: { uv: [0, 0.25, 0.25, 10.25], texture: `#${index}`, tintindex: index },
            south: { uv: [5.5, 0.25, 10.5, 10.25], texture: `#${index}`, tintindex: index },
            west: { uv: [5.25, 0.25, 5.5, 10.25], texture: `#${index}`, tintindex: index },
            up: { uv: [5.25, 0.25, 0.25, 0], texture: `#${index}`, tintindex: index },
            down: { uv: [10.25, 0, 5.25, 0.25], texture: `#${index}`, tintindex: index },
        });
        function createBannerRenderer(color, config) {
            return (atlas, patterns) => {
                const textures = { 0: 'entity/banner_base' };
                const elements = [...config.base];
                const colors = [color];
                patterns?.forEach((compound, index) => {
                    const pattern = Identifier.parse(compound.getString('pattern')).path;
                    const color = compound.getString('color');
                    index++;
                    textures[index] = `entity/banner/${pattern}`;
                    elements.push(config.pattern(index));
                    colors.push(color);
                });
                return new BlockModel(undefined, textures, elements)
                    .getMesh(atlas, Cull.none(), (index) => DyeColors[colors[index]]);
            };
        }
        SpecialRenderers.bannerRenderer = (color) => createBannerRenderer(color, {
            base: [
                {
                    from: [-2, -8, 6],
                    to: [18, 32, 7],
                    faces: bannerFace(0),
                },
                {
                    from: [7, -12, 7],
                    to: [9, 30, 9],
                    faces: {
                        north: { uv: [11.5, 0.5, 12, 11], texture: '#0' },
                        east: { uv: [11, 0.5, 11.5, 11], texture: '#0' },
                        south: { uv: [12.5, 0.5, 13, 11], texture: '#0' },
                        west: { uv: [12, 0.5, 12.5, 11], texture: '#0' },
                        up: { uv: [12, 0.5, 11.5, 0], texture: '#0' },
                        down: { uv: [12.5, 0, 12, 0.5], texture: '#0' },
                    },
                },
                {
                    from: [-2, 30, 7],
                    to: [18, 32, 9],
                    faces: {
                        north: { uv: [0.5, 11, 5.5, 11.5], texture: '#0' },
                        east: { uv: [0, 11, 0.5, 11.5], texture: '#0' },
                        south: { uv: [6, 11, 11, 11.5], texture: '#0' },
                        west: { uv: [5.5, 11, 6, 11.5], texture: '#0' },
                        up: { uv: [5.5, 11, 0.5, 10.5], texture: '#0' },
                        down: { uv: [10.5, 10.5, 5.5, 11], texture: '#0' },
                    },
                },
            ],
            pattern: (index) => ({
                from: [-2, -8, 6],
                to: [18, 32, 7],
                faces: bannerFace(index),
            }),
        });
        SpecialRenderers.wallBannerRenderer = (color) => createBannerRenderer(color, {
            base: [
                {
                    from: [-2, -8, -1.5],
                    to: [18, 32, -0.5],
                    faces: bannerFace(0),
                },
                {
                    from: [-2, 30, -3.5],
                    to: [18, 32, -1.5],
                    faces: {
                        north: { uv: [0.5, 11, 5.5, 11.5], texture: '#0' },
                        east: { uv: [0, 11, 0.5, 11.5], texture: '#0' },
                        south: { uv: [6, 11, 11, 11.5], texture: '#0' },
                        west: { uv: [5.5, 11, 6, 11.5], texture: '#0' },
                        up: { uv: [5.5, 11, 0.5, 10.5], texture: '#0' },
                        down: { uv: [10.5, 10.5, 5.5, 11], texture: '#0' },
                    },
                },
            ],
            pattern: (index) => ({
                from: [-2, -8, -1.5],
                to: [18, 32, -0.5],
                faces: bannerFace(index),
            }),
        });
        function bellRenderer(atlas) {
            return new BlockModel(undefined, {
                0: 'entity/bell/bell_body',
            }, [
                {
                    from: [5, 3, 5],
                    to: [11, 10, 11],
                    faces: {
                        north: { uv: [3, 3, 6, 6.5], texture: '#0' },
                        east: { uv: [0, 3, 3, 6.5], texture: '#0' },
                        south: { uv: [9, 3, 12, 6.5], texture: '#0' },
                        west: { uv: [6, 3, 9, 6.5], texture: '#0' },
                        up: { uv: [6, 3, 3, 0], texture: '#0' },
                        down: { uv: [9, 0, 6, 3], texture: '#0' },
                    },
                },
                {
                    from: [4, 10, 4],
                    to: [12, 12, 12],
                    faces: {
                        north: { uv: [4, 10.5, 8, 11.5], texture: '#0' },
                        east: { uv: [0, 10.5, 4, 11.5], texture: '#0' },
                        south: { uv: [12, 10.5, 16, 11.5], texture: '#0' },
                        west: { uv: [8, 10.5, 12, 11.5], texture: '#0' },
                        up: { uv: [8, 10.5, 4, 6.5], texture: '#0' },
                        down: { uv: [12, 6.5, 8, 10.5], texture: '#0' },
                    },
                },
            ]).getMesh(atlas, Cull.none());
        }
        SpecialRenderers.bellRenderer = bellRenderer;
        function bedRenderer(texture) {
            return (part, atlas) => {
                if (part === 'foot') {
                    return new BlockModel(undefined, {
                        0: texture.withPrefix('entity/bed/').toString(),
                    }, [
                        {
                            from: [0, 3, 0],
                            to: [16, 9, 16],
                            faces: {
                                north: { uv: [5.5, 5.5, 9.5, 7], rotation: 180, texture: '#0' },
                                east: { uv: [0, 7, 1.5, 11], rotation: 270, texture: '#0' },
                                west: { uv: [5.5, 7, 7, 11], rotation: 90, texture: '#0' },
                                up: { uv: [5.5, 11, 1.5, 7], texture: '#0' },
                                down: { uv: [11, 7, 7, 11], texture: '#0' },
                            },
                        },
                        {
                            from: [0, 0, 0],
                            to: [3, 3, 3],
                            faces: {
                                north: { uv: [12.5, 5.25, 13.25, 6], texture: '#0' },
                                east: { uv: [14.75, 5.25, 15.5, 6], texture: '#0' },
                                south: { uv: [14, 5.25, 14.75, 6], texture: '#0' },
                                west: { uv: [13.25, 5.25, 14, 6], texture: '#0' },
                                up: { uv: [13.25, 4.5, 14, 5.25], texture: '#0' },
                                down: { uv: [14, 4.5, 14.75, 5.25], texture: '#0' },
                            },
                        },
                        {
                            from: [13, 0, 0],
                            to: [16, 3, 3],
                            faces: {
                                north: { uv: [13.25, 3.75, 14, 4.5], texture: '#0' },
                                east: { uv: [12.5, 3.75, 13.25, 4.5], texture: '#0' },
                                south: { uv: [14.75, 3.75, 15.5, 4.5], texture: '#0' },
                                west: { uv: [14, 3.75, 14.75, 4.5], texture: '#0' },
                                up: { uv: [13.25, 3, 14, 3.75], texture: '#0' },
                                down: { uv: [14, 3, 14.75, 3.75], texture: '#0' },
                            },
                        },
                    ]).getMesh(atlas, Cull.none());
                }
                return new BlockModel(undefined, {
                    0: texture.withPrefix('entity/bed/').toString(),
                }, [
                    {
                        from: [0, 3, 0],
                        to: [16, 9, 16],
                        faces: {
                            east: { uv: [0, 1.5, 1.5, 5.5], rotation: 270, texture: '#0' },
                            south: { uv: [1.5, 0, 5.5, 1.5], rotation: 180, texture: '#0' },
                            west: { uv: [5.5, 1.5, 7, 5.5], rotation: 90, texture: '#0' },
                            up: { uv: [5.5, 5.5, 1.5, 1.5], texture: '#0' },
                            down: { uv: [11, 1.5, 7, 5.5], texture: '#0' },
                        },
                    },
                    {
                        from: [0, 0, 13],
                        to: [3, 3, 16],
                        faces: {
                            north: { uv: [14.75, 0.75, 15.5, 1.5], texture: '#0' },
                            east: { uv: [14, 0.75, 14.75, 1.5], texture: '#0' },
                            south: { uv: [13.25, 0.75, 14, 1.5], texture: '#0' },
                            west: { uv: [12.5, 0.75, 13.25, 1.5], texture: '#0' },
                            up: { uv: [13.25, 0, 14, 0.75], texture: '#0' },
                            down: { uv: [14, 0, 14.75, 0.75], texture: '#0' },
                        },
                    },
                    {
                        from: [13, 0, 13],
                        to: [16, 3, 16],
                        faces: {
                            north: { uv: [14, 2.25, 14.75, 3], texture: '#0' },
                            east: { uv: [13.25, 2.25, 14, 3], texture: '#0' },
                            south: { uv: [12.5, 2.25, 13.25, 3], texture: '#0' },
                            west: { uv: [14.75, 2.25, 15.5, 3], texture: '#0' },
                            up: { uv: [13.25, 1.5, 14, 2.25], texture: '#0' },
                            down: { uv: [14, 1.5, 14.75, 2.25], texture: '#0' },
                        },
                    },
                ]).getMesh(atlas, Cull.none());
            };
        }
        SpecialRenderers.bedRenderer = bedRenderer;
        function getStr(block, key, fallback = '') {
            return block.getProperty(key) ?? fallback;
        }
        function getInt(block, key, fallback = '0') {
            return parseInt(block.getProperty(key) ?? fallback);
        }
        const ChestRenderers = new Map(Object.entries({
            'minecraft:chest': SpecialRenderers.chestRenderer(Identifier.create('normal')),
            'minecraft:ender_chest': SpecialRenderers.chestRenderer(Identifier.create('ender')),
            'minecraft:trapped_chest': SpecialRenderers.chestRenderer(Identifier.create('trapped')),
            'minecraft:copper_chest': SpecialRenderers.chestRenderer(Identifier.create('copper')),
            'minecraft:exposed_copper_chest': SpecialRenderers.chestRenderer(Identifier.create('copper_exposed')),
            'minecraft:weathered_copper_chest': SpecialRenderers.chestRenderer(Identifier.create('copper_weathered')),
            'minecraft:oxidized_copper_chest': SpecialRenderers.chestRenderer(Identifier.create('copper_oxidized')),
            'minecraft:waxed_copper_chest': SpecialRenderers.chestRenderer(Identifier.create('copper')),
            'minecraft:waxed_exposed_copper_chest': SpecialRenderers.chestRenderer(Identifier.create('copper_exposed')),
            'minecraft:waxed_weathered_copper_chest': SpecialRenderers.chestRenderer(Identifier.create('copper_weathered')),
            'minecraft:waxed_oxidized_copper_chest': SpecialRenderers.chestRenderer(Identifier.create('copper_oxidized')),
        }));
        const SkullRenderers = new Map(Object.entries({
            'minecraft:skeleton_skull': SpecialRenderers.headRenderer(Identifier.create('skeleton/skeleton'), 2),
            'minecraft:wither_skeleton_skull': SpecialRenderers.headRenderer(Identifier.create('skeleton/wither_skeleton'), 2),
            'minecraft:zombie_head': SpecialRenderers.headRenderer(Identifier.create('zombie/zombie'), 1),
            'minecraft:creeper_head': SpecialRenderers.headRenderer(Identifier.create('creeper/creeper'), 2),
            'minecraft:dragon_head': SpecialRenderers.dragonHeadRenderer(),
            'minecraft:piglin_head': SpecialRenderers.piglinHeadRenderer(),
            'minecraft:player_head': SpecialRenderers.headRenderer(Identifier.create('player/wide/steve'), 1), // TODO: fix texture
        }));
        const WoodTypes = [
            'oak',
            'spruce',
            'birch',
            'jungle',
            'acacia',
            'dark_oak',
            'mangrove',
            'cherry',
            'bamboo',
            'crimson',
            'warped',
        ];
        const SignRenderers = new Map(WoodTypes.map(type => [`minecraft:${type}_sign`, SpecialRenderers.signRenderer(Identifier.create(type))]));
        const WallSignRenderers = new Map(WoodTypes.map(type => [`minecraft:${type}_wall_sign`, SpecialRenderers.wallSignRenderer(Identifier.create(type))]));
        const HangingSignRenderers = new Map(WoodTypes.map(type => [`minecraft:${type}_hanging_sign`, SpecialRenderers.hangingSignRenderer(Identifier.create(type))]));
        const WallHangingSignRenderers = new Map(WoodTypes.map(type => [`minecraft:${type}_wall_hanging_sign`, SpecialRenderers.wallHangingSignRenderer(type)]));
        const ShulkerBoxRenderers = new Map(Object.keys(DyeColors).map(color => [`minecraft:${color}_shulker_box`, SpecialRenderers.shulkerBoxRenderer(Identifier.create(`shulker_${color}`))]));
        const BedRenderers = new Map(Object.keys(DyeColors).map(color => [`minecraft:${color}_bed`, SpecialRenderers.bedRenderer(Identifier.create(color))]));
        const BannerRenderers = new Map(Object.keys(DyeColors).map(color => [`minecraft:${color}_banner`, SpecialRenderers.bannerRenderer(color)]));
        const WallBannerRenderers = new Map(Object.keys(DyeColors).map(color => [`minecraft:${color}_wall_banner`, SpecialRenderers.wallBannerRenderer(color)]));
        function getBlockMesh(block, nbt, atlas, cull) {
            const mesh = new Mesh();
            if (block.is('water')) {
                mesh.merge(liquidRenderer('water', getInt(block, 'level'), atlas, cull, 0));
            }
            if (block.is('lava')) {
                mesh.merge(liquidRenderer('lava', getInt(block, 'level'), atlas, cull));
            }
            const chestRenderer = ChestRenderers.get(block.getName().toString());
            if (chestRenderer !== undefined) {
                const facing = getStr(block, 'facing', 'south');
                const t = create$2();
                translate(t, t, [8, 8, 8]);
                rotateY$1(t, t, facing === 'west' ? Math.PI / 2 : facing === 'south' ? Math.PI : facing === 'east' ? Math.PI * 3 / 2 : 0);
                translate(t, t, [-8, -8, -8]);
                mesh.merge(chestRenderer(atlas).transform(t));
            }
            if (block.is('decorated_pot')) {
                mesh.merge(decoratedPotRenderer(atlas));
            }
            const skullRenderer = SkullRenderers.get(block.getName().toString());
            if (skullRenderer !== undefined) {
                const rotation = getInt(block, 'rotation') / 16 * Math.PI * 2;
                const t = create$2();
                translate(t, t, [8, 8, 8]);
                rotateY$1(t, t, rotation);
                translate(t, t, [-8, -8, -8]);
                mesh.merge(skullRenderer(atlas).transform(t));
            }
            const signRenderer = SignRenderers.get(block.getName().toString());
            if (signRenderer !== undefined) {
                const rotation = getInt(block, 'rotation') / 16 * Math.PI * 2;
                const t = create$2();
                translate(t, t, [8, 8, 8]);
                rotateY$1(t, t, rotation);
                scale$1(t, t, [2 / 3, 2 / 3, 2 / 3]);
                translate(t, t, [-8, -8, -8]);
                mesh.merge(signRenderer(atlas).transform(t));
            }
            const wallSignRenderer = WallSignRenderers.get(block.getName().toString());
            if (wallSignRenderer !== undefined) {
                const facing = getStr(block, 'facing', 'south');
                const t = create$2();
                translate(t, t, [8, 8, 8]);
                rotateY$1(t, t, facing === 'west' ? Math.PI / 2 : facing === 'south' ? Math.PI : facing === 'east' ? Math.PI * 3 / 2 : 0);
                scale$1(t, t, [2 / 3, 2 / 3, 2 / 3]);
                translate(t, t, [-8, -8, -8]);
                mesh.merge(wallSignRenderer(atlas).transform(t));
            }
            const hangingSignRenderer = HangingSignRenderers.get(block.getName().toString());
            if (hangingSignRenderer !== undefined) {
                const attached = getStr(block, 'attached', 'false') === 'true';
                const rotation = getInt(block, 'rotation') / 16 * Math.PI * 2;
                const t = create$2();
                translate(t, t, [8, 8, 8]);
                rotateY$1(t, t, rotation);
                scale$1(t, t, [2 / 3, 2 / 3, 2 / 3]);
                translate(t, t, [-8, -8, -8]);
                mesh.merge(hangingSignRenderer(attached, atlas).transform(t));
            }
            const wallHangingSignRenderer = WallHangingSignRenderers.get(block.getName().toString());
            if (wallHangingSignRenderer !== undefined) {
                const facing = getStr(block, 'facing', 'south');
                const t = create$2();
                translate(t, t, [8, 8, 8]);
                rotateY$1(t, t, facing === 'west' ? Math.PI / 2 : facing === 'south' ? Math.PI : facing === 'east' ? Math.PI * 3 / 2 : 0);
                translate(t, t, [-8, -8, -8]);
                mesh.merge(wallHangingSignRenderer(atlas).transform(t));
            }
            if (block.is('conduit')) {
                mesh.merge(conduitRenderer(atlas));
            }
            const shulkerBoxRenderer = ShulkerBoxRenderers.get(block.getName().toString());
            if (shulkerBoxRenderer !== undefined) {
                const facing = getStr(block, 'facing', 'up');
                const t = create$2();
                translate(t, t, [8, 8, 8]);
                if (facing === 'down') {
                    rotateX$1(t, t, Math.PI);
                }
                else if (facing !== 'up') {
                    rotateY$1(t, t, facing === 'east' ? Math.PI / 2 : facing === 'north' ? Math.PI : facing === 'west' ? Math.PI * 3 / 2 : 0);
                    rotateX$1(t, t, Math.PI / 2);
                }
                translate(t, t, [-8, -8, -8]);
                mesh.merge(shulkerBoxRenderer(atlas).transform(t));
            }
            if (block.is('bell')) {
                const t = create$2();
                translate(t, t, [8, 8, 8]);
                scale$1(t, t, [1, -1, -1]);
                translate(t, t, [-8, -8, -8]);
                mesh.merge(bellRenderer(atlas).transform(t));
            }
            const bedRenderer = BedRenderers.get(block.getName().toString());
            if (bedRenderer !== undefined) {
                const part = getStr(block, 'part', 'head');
                const facing = getStr(block, 'facing', 'south');
                const t = create$2();
                translate(t, t, [8, 8, 8]);
                rotateY$1(t, t, facing === 'east' ? Math.PI / 2 : facing === 'north' ? Math.PI : facing === 'west' ? Math.PI * 3 / 2 : 0);
                translate(t, t, [-8, -8, -8]);
                mesh.merge(bedRenderer(part, atlas).transform(t));
            }
            const bannerRenderer = BannerRenderers.get(block.getName().toString());
            if (bannerRenderer !== undefined) {
                const rotation = getInt(block, 'rotation') / 16 * Math.PI * 2;
                const t = create$2();
                translate(t, t, [8, 24, 8]);
                rotateY$1(t, t, rotation);
                scale$1(t, t, [2 / 3, 2 / 3, 2 / 3]);
                translate(t, t, [-8, -24, -8]);
                mesh.merge(bannerRenderer(atlas, nbt?.getList('patterns', NbtType.Compound)).transform(t));
            }
            const wallBannerRenderer = WallBannerRenderers.get(block.getName().toString());
            if (wallBannerRenderer !== undefined) {
                const facing = getStr(block, 'facing', 'south');
                const t = create$2();
                translate(t, t, [8, 8, 8]);
                rotateY$1(t, t, facing === 'east' ? Math.PI / 2 : facing === 'north' ? Math.PI : facing === 'west' ? Math.PI * 3 / 2 : 0);
                scale$1(t, t, [2 / 3, 2 / 3, 2 / 3]);
                translate(t, t, [-8, -23.2, -8]);
                mesh.merge(wallBannerRenderer(atlas, nbt?.getList('patterns', NbtType.Compound)).transform(t));
            }
            if (!block.is('water') && !block.is('lava') && block.isWaterlogged()) {
                mesh.merge(liquidRenderer('water', 0, atlas, cull, 0));
            }
            const t = create$2();
            scale$1(t, t, [0.0625, 0.0625, 0.0625]);
            return mesh.transform(t);
        }
        SpecialRenderers.getBlockMesh = getBlockMesh;
    })(SpecialRenderers || (SpecialRenderers = {}));

    class ChunkBuilder {
        gl;
        structure;
        resources;
        chunks = [];
        chunkSize;
        constructor(gl, structure, resources, chunkSize = 16) {
            this.gl = gl;
            this.structure = structure;
            this.resources = resources;
            this.chunkSize = typeof chunkSize === 'number' ? [chunkSize, chunkSize, chunkSize] : chunkSize;
            this.updateStructureBuffers();
        }
        setStructure(structure) {
            this.structure = structure;
            this.updateStructureBuffers();
        }
        updateStructureBuffers(chunkPositions) {
            if (!this.structure)
                return;
            if (!chunkPositions) {
                this.chunks.forEach(x => x.forEach(y => y.forEach(chunk => {
                    chunk.mesh.clear();
                    chunk.transparentMesh.clear();
                })));
            }
            else {
                chunkPositions.forEach(chunkPos => {
                    const chunk = this.getChunk(chunkPos);
                    chunk.mesh.clear();
                    chunk.transparentMesh.clear();
                });
            }
            for (const b of this.structure.getBlocks()) {
                const blockName = b.state.getName();
                const blockProps = b.state.getProperties();
                const defaultProps = this.resources.getDefaultBlockProperties(blockName) ?? {};
                Object.entries(defaultProps).forEach(([k, v]) => {
                    if (!blockProps[k])
                        blockProps[k] = v;
                });
                const chunkPos = [Math.floor(b.pos[0] / this.chunkSize[0]), Math.floor(b.pos[1] / this.chunkSize[1]), Math.floor(b.pos[2] / this.chunkSize[2])];
                if (chunkPositions && !chunkPositions.some(pos => equals(pos, chunkPos)))
                    continue;
                const chunk = this.getChunk(chunkPos);
                try {
                    const blockDefinition = this.resources.getBlockDefinition(blockName);
                    const cull = {
                        up: this.needsCull(b, Direction.UP),
                        down: this.needsCull(b, Direction.DOWN),
                        west: this.needsCull(b, Direction.WEST),
                        east: this.needsCull(b, Direction.EAST),
                        north: this.needsCull(b, Direction.NORTH),
                        south: this.needsCull(b, Direction.SOUTH),
                    };
                    const mesh = new Mesh();
                    if (blockDefinition) {
                        mesh.merge(blockDefinition.getMesh(blockName, blockProps, this.resources, this.resources, cull));
                    }
                    const specialMesh = SpecialRenderers.getBlockMesh(b.state, b.nbt, this.resources, cull);
                    if (!specialMesh.isEmpty()) {
                        mesh.merge(specialMesh);
                    }
                    if (!mesh.isEmpty()) {
                        this.finishChunkMesh(mesh, b.pos);
                        if (this.resources.getBlockFlags(b.state.getName())?.semi_transparent) {
                            chunk.transparentMesh.merge(mesh);
                        }
                        else {
                            chunk.mesh.merge(mesh);
                        }
                    }
                }
                catch (e) {
                    console.error(`Error rendering block ${blockName}`, e);
                }
            }
            if (!chunkPositions) {
                this.chunks.forEach(x => x.forEach(y => y.forEach(chunk => {
                    chunk.mesh.rebuild(this.gl, { pos: true, color: true, texture: true, normal: true, blockPos: true });
                    chunk.transparentMesh.rebuild(this.gl, { pos: true, color: true, texture: true, normal: true, blockPos: true });
                })));
            }
            else {
                chunkPositions.forEach(chunkPos => {
                    const chunk = this.getChunk(chunkPos);
                    chunk.mesh.rebuild(this.gl, { pos: true, color: true, texture: true, normal: true, blockPos: true });
                    chunk.transparentMesh.rebuild(this.gl, { pos: true, color: true, texture: true, normal: true, blockPos: true });
                });
            }
        }
        getMeshes() {
            const chunks = this.chunks.flatMap(x => x.flatMap(y => y.flatMap(chunk => chunk ?? [])));
            return chunks.flatMap(chunk => chunk.mesh.isEmpty() ? [] : chunk.mesh).concat(chunks.flatMap(chunk => chunk.transparentMesh.isEmpty() ? [] : chunk.transparentMesh));
        }
        needsCull(block, dir) {
            const neighbor = this.structure.getBlock(BlockPos.towards(block.pos, dir))?.state;
            if (!neighbor)
                return false;
            const neighborFlags = this.resources.getBlockFlags(neighbor.getName());
            if (block.state.getName().equals(neighbor.getName()) && neighborFlags?.self_culling) {
                return true;
            }
            if (neighborFlags?.opaque) {
                return !(dir === Direction.UP && block.state.isWaterlogged());
            }
            else {
                return block.state.isWaterlogged() && neighbor.isWaterlogged();
            }
        }
        finishChunkMesh(mesh, pos) {
            const t = create$2();
            translate(t, t, pos);
            mesh.transform(t);
            for (const q of mesh.quads) {
                const normal = q.normal();
                q.forEach(v => v.normal = normal);
                q.forEach(v => v.blockPos = new Vector(pos[0], pos[1], pos[2]));
            }
        }
        getChunk(chunkPos) {
            const x = Math.abs(chunkPos[0]) * 2 + (chunkPos[0] < 0 ? 1 : 0);
            const y = Math.abs(chunkPos[1]) * 2 + (chunkPos[1] < 0 ? 1 : 0);
            const z = Math.abs(chunkPos[2]) * 2 + (chunkPos[2] < 0 ? 1 : 0);
            if (!this.chunks[x])
                this.chunks[x] = [];
            if (!this.chunks[x][y])
                this.chunks[x][y] = [];
            if (!this.chunks[x][y][z])
                this.chunks[x][y][z] = { mesh: new Mesh(), transparentMesh: new Mesh() };
            return this.chunks[x][y][z];
        }
    }

    class ShaderProgram {
        gl;
        program;
        constructor(gl, vsSource, fsSource) {
            this.gl = gl;
            this.program = this.initShaderProgram(vsSource, fsSource);
        }
        getProgram() {
            return this.program;
        }
        initShaderProgram(vsSource, fsSource) {
            const vertexShader = this.loadShader(this.gl.VERTEX_SHADER, vsSource);
            const fragmentShader = this.loadShader(this.gl.FRAGMENT_SHADER, fsSource);
            const shaderProgram = this.gl.createProgram();
            this.gl.attachShader(shaderProgram, vertexShader);
            this.gl.attachShader(shaderProgram, fragmentShader);
            this.gl.linkProgram(shaderProgram);
            if (!this.gl.getProgramParameter(shaderProgram, this.gl.LINK_STATUS)) {
                throw new Error(`Unable to link shader program: ${this.gl.getProgramInfoLog(shaderProgram)}`);
            }
            return shaderProgram;
        }
        loadShader(type, source) {
            const shader = this.gl.createShader(type);
            this.gl.shaderSource(shader, source);
            this.gl.compileShader(shader);
            if (!this.gl.getShaderParameter(shader, this.gl.COMPILE_STATUS)) {
                const error = new Error(`Compiling ${type === this.gl.VERTEX_SHADER ? 'vertex' : 'fragment'} shader: ${this.gl.getShaderInfoLog(shader)}`);
                this.gl.deleteShader(shader);
                throw error;
            }
            return shader;
        }
    }

    const vsSource = `
  attribute vec4 vertPos;
  attribute vec2 texCoord;
  attribute vec4 texLimit;
  attribute vec3 vertColor;
  attribute vec3 normal;

  uniform mat4 mView;
  uniform mat4 mProj;

  varying highp vec2 vTexCoord;
  varying highp vec4 vTexLimit;
  varying highp vec3 vTintColor;
  varying highp float vLighting;

  void main(void) {
    gl_Position = mProj * mView * vertPos;
    vTexCoord = texCoord;
	vTexLimit = texLimit;
    vTintColor = vertColor;
    vLighting = normal.y * 0.2 + abs(normal.z) * 0.1 + 0.8;
  }
`;
    const fsSource = `
  precision highp float;
  varying highp vec2 vTexCoord;
  varying highp vec4 vTexLimit;
  varying highp vec3 vTintColor;
  varying highp float vLighting;

  uniform sampler2D sampler;
  uniform highp float pixelSize;

  void main(void) {
		vec4 texColor = texture2D(sampler, clamp(vTexCoord,
			vTexLimit.xy + vec2(0.5, 0.5) * pixelSize,
			vTexLimit.zw - vec2(0.5, 0.5) * pixelSize
		));
		if(texColor.a < 0.01) discard;
		gl_FragColor = vec4(texColor.xyz * vTintColor * vLighting, texColor.a);
  }
`;
    class Renderer {
        gl;
        shaderProgram;
        projMatrix;
        activeShader;
        pixelSize = 0;
        constructor(gl) {
            this.gl = gl;
            this.shaderProgram = new ShaderProgram(gl, vsSource, fsSource).getProgram();
            this.activeShader = this.shaderProgram;
            this.projMatrix = this.getPerspective();
            this.initialize();
        }
        setViewport(x, y, width, height) {
            this.gl.viewport(x, y, width, height);
            this.projMatrix = this.getPerspective();
        }
        getPerspective() {
            const c = this.gl.canvas;
            const bw = Math.max(1, c.width || c.clientWidth || 1);
            const bh = Math.max(1, c.height || c.clientHeight || 1);
            const aspect = bw / bh;
            const projMatrix = create$2();
            const fovDeg = typeof this.lbaFovDeg === 'number' ? this.lbaFovDeg : 70;
            if (fovDeg <= 0) {
                const halfH = typeof this.lbaOrthoHalfHeight === 'number' && this.lbaOrthoHalfHeight > 0
                    ? this.lbaOrthoHalfHeight
                    : 8;
                const halfW = halfH * aspect;
                ortho(projMatrix, -halfW, halfW, -halfH, halfH, 0.1, 500.0);
            }
            else {
                const clamped = Math.max(1, Math.min(110, fovDeg));
                const fieldOfView = clamped * Math.PI / 180;
                perspective(projMatrix, fieldOfView, aspect, 0.1, 500.0);
            }
            return projMatrix;
        }
        initialize() {
            this.gl.enable(this.gl.DEPTH_TEST);
            this.gl.depthFunc(this.gl.LEQUAL);
            this.gl.enable(this.gl.BLEND);
            this.gl.blendFunc(this.gl.SRC_ALPHA, this.gl.ONE_MINUS_SRC_ALPHA);
            this.gl.enable(this.gl.CULL_FACE);
            this.gl.cullFace(this.gl.BACK);
        }
        setShader(shader) {
            this.gl.useProgram(shader);
            this.activeShader = shader;
        }
        setVertexAttr(name, size, buffer) {
            if (buffer === undefined)
                throw new Error(`Expected buffer for ${name}`);
            const location = this.gl.getAttribLocation(this.activeShader, name);
            if (location < 0)
                return;
            this.gl.bindBuffer(this.gl.ARRAY_BUFFER, buffer);
            this.gl.vertexAttribPointer(location, size, this.gl.FLOAT, false, 0, 0);
            this.gl.enableVertexAttribArray(location);
        }
        setUniform(name, value) {
            const location = this.gl.getUniformLocation(this.activeShader, name);
            this.gl.uniformMatrix4fv(location, false, value);
        }
        setTexture(texture, pixelSize) {
            this.gl.activeTexture(this.gl.TEXTURE0);
            this.gl.bindTexture(this.gl.TEXTURE_2D, texture);
            this.pixelSize = pixelSize ?? 0;
        }
        createAtlasTexture(image) {
            const texture = this.gl.createTexture();
            this.gl.bindTexture(this.gl.TEXTURE_2D, texture);
            this.gl.texImage2D(this.gl.TEXTURE_2D, 0, this.gl.RGBA, this.gl.RGBA, this.gl.UNSIGNED_BYTE, image);
            this.gl.generateMipmap(this.gl.TEXTURE_2D);
            this.gl.texParameteri(this.gl.TEXTURE_2D, this.gl.TEXTURE_MAG_FILTER, this.gl.NEAREST);
            return texture;
        }
        prepareDraw(viewMatrix) {
            this.setUniform('mView', viewMatrix);
            this.setUniform('mProj', this.projMatrix);
            const location = this.gl.getUniformLocation(this.activeShader, 'pixelSize');
            this.gl.uniform1f(location, this.pixelSize);
        }
        drawMesh(mesh, options) {
            if (mesh.quadVertices() > 0) {
                if (options.pos)
                    this.setVertexAttr('vertPos', 3, mesh.posBuffer);
                if (options.color)
                    this.setVertexAttr('vertColor', 3, mesh.colorBuffer);
                if (options.texture) {
                    this.setVertexAttr('texCoord', 2, mesh.textureBuffer);
                    this.setVertexAttr('texLimit', 4, mesh.textureLimitBuffer);
                }
                if (options.normal)
                    this.setVertexAttr('normal', 3, mesh.normalBuffer);
                if (options.blockPos)
                    this.setVertexAttr('blockPos', 3, mesh.blockPosBuffer);
                if (!mesh.indexBuffer)
                    throw new Error('Expected index buffer');
                this.gl.bindBuffer(this.gl.ELEMENT_ARRAY_BUFFER, mesh.indexBuffer);
                this.gl.drawElements(this.gl.TRIANGLES, mesh.quadIndices(), this.gl.UNSIGNED_SHORT, 0);
            }
            if (mesh.lineVertices() > 0) {
                if (options.pos)
                    this.setVertexAttr('vertPos', 3, mesh.linePosBuffer);
                if (options.color)
                    this.setVertexAttr('vertColor', 3, mesh.lineColorBuffer);
                this.gl.drawArrays(this.gl.LINES, 0, mesh.lineVertices());
            }
        }
    }

    class ItemRenderer extends Renderer {
        item;
        resources;
        mesh;
        atlasTexture;
        constructor(gl, item, resources, context = {}) {
            super(gl);
            this.item = item;
            this.resources = resources;
            this.updateMesh(context);
            this.atlasTexture = this.createAtlasTexture(this.resources.getTextureAtlas());
        }
        setItem(item, context = {}) {
            this.item = item;
            this.updateMesh(context);
        }
        updateMesh(context = {}) {
            this.mesh = ItemRenderer.getItemMesh(this.item, this.resources, context);
            this.mesh.computeNormals();
            this.mesh.rebuild(this.gl, { pos: true, color: true, texture: true, normal: true });
        }
        static getItemMesh(item, resources, context) {
            const itemModelId = item.getComponent('item_model', resources)?.getAsString();
            if (itemModelId === undefined) {
                return new Mesh();
            }
            const itemModel = resources.getItemModel(Identifier.parse(itemModelId));
            if (!itemModel) {
                throw new Error(`Item model ${itemModelId} does not exist (defined by item ${item.toString()})`);
            }
            const mesh = itemModel.getMesh(item, resources, context);
            return mesh;
        }
        getPerspective() {
            const projMatrix = create$2();
            ortho(projMatrix, 0, 16, 0, 16, 0.1, 500.0);
            return projMatrix;
        }
        drawItem() {
            const view = create$2();
            translate(view, view, [0, 0, -32]);
            this.setShader(this.shaderProgram);
            this.setTexture(this.atlasTexture, this.resources.getPixelSize?.());
            this.prepareDraw(view);
            this.drawMesh(this.mesh, { pos: true, color: true, texture: true, normal: true });
        }
    }

    const INVALID_COLOR = [0, 0, 0];
    var ItemTint;
    (function (ItemTint) {
        function fromJson(obj) {
            const root = Json.readObject(obj) ?? {};
            const type = Json.readString(root.type)?.replace(/^minecraft:/, '');
            switch (type) {
                case 'constant': return new Constant(Color.fromJson(root.value) ?? INVALID_COLOR);
                case 'dye': return new Dye(Color.fromJson(root.default) ?? INVALID_COLOR);
                case 'grass': return new Grass(Json.readNumber(root.temperature) ?? 0, Json.readNumber(root.downfall) ?? 0);
                case 'firework': return new Firework(Color.fromJson(root.default) ?? INVALID_COLOR);
                case 'potion': return new Potion(Color.fromJson(root.default) ?? INVALID_COLOR);
                case 'map_color': return new MapColor(Color.fromJson(root.default) ?? INVALID_COLOR);
                case 'custom_model_data': return new CustomModelData(Json.readInt(root.index) ?? 0, Color.fromJson(root.default) ?? INVALID_COLOR);
                case 'team': return new Team(Color.fromJson(root.default) ?? INVALID_COLOR);
                default:
                    throw new Error(`Invalid item tint type ${type}`);
            }
        }
        ItemTint.fromJson = fromJson;
        class Constant {
            value;
            constructor(value) {
                this.value = value;
            }
            getTint(item) {
                return this.value;
            }
        }
        ItemTint.Constant = Constant;
        class Dye {
            default_color;
            constructor(default_color) {
                this.default_color = default_color;
            }
            getTint(item, resources) {
                const tag = item.getComponent('dyed_color', resources);
                if (!tag) {
                    return this.default_color;
                }
                if (!tag.isCompound()) {
                    return Color.intToRgb(tag.getAsNumber());
                }
                return Color.intToRgb(tag.getNumber('rgb'));
            }
        }
        ItemTint.Dye = Dye;
        class Grass {
            temperature;
            downfall;
            constructor(temperature, downfall) {
                this.temperature = temperature;
                this.downfall = downfall;
            }
            getTint(item) {
                return [124 / 255, 189 / 255, 107 / 255]; // TODO: this is hardcoded to the same value as for blocks
            }
        }
        ItemTint.Grass = Grass;
        class Firework {
            default_color;
            constructor(default_color) {
                this.default_color = default_color;
            }
            getTint(item, resources) {
                const tag = item.getComponent('firework_explosion', resources);
                if (!tag?.isCompound()) {
                    return this.default_color;
                }
                const colors = tag.get('colors');
                if (!colors || !colors.isListOrArray()) {
                    return this.default_color;
                }
                const color = (() => {
                    if (colors.length === 1) {
                        return Color.intToRgb(colors.get(0).getAsNumber());
                    }
                    let [r, g, b] = [0, 0, 0];
                    for (const color of colors.getItems()) {
                        r += (color.getAsNumber() & 0xFF0000) >> 16;
                        g += (color.getAsNumber() & 0xFF00) >> 8;
                        b += (color.getAsNumber() & 0xFF) >> 0;
                    }
                    r /= colors.length;
                    g /= colors.length;
                    b /= colors.length;
                    return [r / 255, g / 255, b / 255];
                })();
                return color;
            }
        }
        ItemTint.Firework = Firework;
        class Potion {
            default_color;
            constructor(default_color) {
                this.default_color = default_color;
            }
            getTint(item, resources) {
                const tag = item.getComponent('potion_contents', resources);
                if (!tag) {
                    return this.default_color;
                }
                const potionContents = PotionContents.fromNbt(tag);
                return PotionContents.getColor(potionContents);
            }
        }
        ItemTint.Potion = Potion;
        class MapColor {
            default_color;
            constructor(default_color) {
                this.default_color = default_color;
            }
            getTint(item, resources) {
                const mapColor = item.getComponent('map_color', resources);
                if (!mapColor) {
                    return this.default_color;
                }
                return Color.intToRgb(mapColor.getAsNumber());
            }
        }
        ItemTint.MapColor = MapColor;
        class CustomModelData {
            index;
            default_color;
            constructor(index, default_color) {
                this.index = index;
                this.default_color = default_color;
            }
            getTint(item, resources) {
                const tag = item.getComponent('custom_model_data', resources);
                if (!tag?.isCompound()) {
                    return this.default_color;
                }
                const colors = tag.getList('colors').get(this.index);
                if (!colors) {
                    return this.default_color;
                }
                return Color.fromNbt(colors) ?? this.default_color;
            }
        }
        ItemTint.CustomModelData = CustomModelData;
        class Team {
            default_color;
            constructor(default_color) {
                this.default_color = default_color;
            }
            getTint(item, resources, context) {
                return context.context_entity_team_color ?? this.default_color;
            }
        }
        ItemTint.Team = Team;
    })(ItemTint || (ItemTint = {}));

    var SpecialModel;
    (function (SpecialModel) {
        function fromJson(obj) {
            const root = Json.readObject(obj) ?? {};
            const type = Json.readString(root.type)?.replace(/^minecraft:/, '');
            switch (type) {
                case 'bed': return new Bed(Identifier.parse(Json.readString(root.texture) ?? ''));
                case 'banner': return new Banner(Json.readString(root.color) ?? '');
                case 'conduit': return new Conduit();
                case 'chest': return new Chest(Identifier.parse(Json.readString(root.texture) ?? ''), Json.readNumber(root.openness) ?? 0);
                case 'head': return new Head(Json.readString(root.kind) ?? '', typeof root.texture === 'string' ? Identifier.parse(root.texture) : undefined, Json.readNumber(root.animation) ?? 0);
                case 'player_head': return new Head('player', undefined, 0);
                case 'shulker_box': return new ShulkerBox(Identifier.parse(Json.readString(root.texture) ?? ''), Json.readNumber(root.openness) ?? 0, (Json.readString(root.orientation) ?? 'up'));
                case 'shield': return new Shield();
                case 'trident': return new Trident();
                case 'decorated_pot': return new DecoratedPot();
                case 'standing_sign': return new StandingSign(Json.readString(root.wood_type) ?? '', typeof root.texture === 'string' ? Identifier.parse(root.texture) : undefined);
                case 'hanging_sign': return new HangingSign(Json.readString(root.wood_type) ?? '', typeof root.texture === 'string' ? Identifier.parse(root.texture) : undefined);
                default:
                    console.warn(`[deepslate]: Unknown special model ${type}`);
                    return { getMesh: () => new Mesh() };
            }
        }
        SpecialModel.fromJson = fromJson;
        class Bed {
            renderer;
            constructor(texture) {
                this.renderer = SpecialRenderers.bedRenderer(texture);
            }
            getMesh(item, resources) {
                const headMesh = this.renderer('head', resources);
                const footMesh = this.renderer('foot', resources);
                const t = create$2();
                translate(t, t, [0, 0, -16]);
                return headMesh.merge(footMesh.transform(t));
            }
        }
        class Banner {
            renderer;
            constructor(color) {
                this.renderer = SpecialRenderers.bannerRenderer(color);
            }
            getMesh(item, resources) {
                const patterns = item.getComponent('banner_patterns', undefined);
                const t = create$2();
                translate(t, t, [8, 24, 8]);
                rotateY$1(t, t, Math.PI);
                scale$1(t, t, [2 / 3, 2 / 3, 2 / 3]);
                translate(t, t, [-8, -24, -8]);
                return this.renderer(resources, patterns instanceof (NbtList) ? patterns : undefined).transform(t);
            }
        }
        class Conduit {
            getMesh(item, resources) {
                return SpecialRenderers.conduitRenderer(resources);
            }
        }
        class Chest {
            renderer;
            constructor(texture, openness) {
                this.renderer = SpecialRenderers.chestRenderer(texture);
            }
            getMesh(item, resources) {
                const t = create$2();
                translate(t, t, [8, 8, 8]);
                rotateY$1(t, t, Math.PI);
                translate(t, t, [-8, -8, -8]);
                return this.renderer(resources).transform(t);
            }
        }
        class Head {
            renderer;
            constructor(kind, texture, animation) {
                this.renderer = ({
                    skeleton: () => SpecialRenderers.headRenderer(texture ?? Identifier.create('skeleton/skeleton'), 2),
                    wither_skeleton: () => SpecialRenderers.headRenderer(texture ?? Identifier.create('skeleton/wither_skeleton'), 2),
                    zombie: () => SpecialRenderers.headRenderer(texture ?? Identifier.create('zombie/zombie'), 1),
                    creeper: () => SpecialRenderers.headRenderer(texture ?? Identifier.create('creeper/creeper'), 2),
                    dragon: () => SpecialRenderers.dragonHeadRenderer(texture),
                    piglin: () => SpecialRenderers.piglinHeadRenderer(texture),
                    player: () => SpecialRenderers.headRenderer(texture ?? Identifier.create('player/wide/steve'), 1), // TODO: fix texture
                }[kind] ?? (() => () => new Mesh()))();
            }
            getMesh(item, resources) {
                return this.renderer(resources);
            }
        }
        class ShulkerBox {
            renderer;
            constructor(texture, openness, orientation) {
                this.renderer = SpecialRenderers.shulkerBoxRenderer(texture);
            }
            getMesh(item, resources) {
                return this.renderer(resources);
            }
        }
        class Shield {
            getMesh(item, resources) {
                const shieldMesh = SpecialRenderers.shieldRenderer(resources);
                const t = create$2();
                translate(t, t, [-3, 1, 0]);
                rotateX$1(t, t, -10 * Math.PI / 180);
                rotateY$1(t, t, -10 * Math.PI / 180);
                rotateZ(t, t, -5 * Math.PI / 180);
                return shieldMesh.transform(t);
            }
        }
        class Trident {
            getMesh(item, resources) {
                return new Mesh(); // TODO
            }
        }
        class DecoratedPot {
            getMesh(item, resources) {
                return SpecialRenderers.decoratedPotRenderer(resources);
            }
        }
        class StandingSign {
            renderer;
            constructor(wood_type, texture) {
                this.renderer = SpecialRenderers.signRenderer(texture ?? Identifier.create(wood_type));
            }
            getMesh(item, resources) {
                return this.renderer(resources);
            }
        }
        class HangingSign {
            renderer;
            constructor(wood_type, texture) {
                this.renderer = SpecialRenderers.hangingSignRenderer(texture ?? Identifier.create(wood_type));
            }
            getMesh(item, resources) {
                return this.renderer(false, resources);
            }
        }
    })(SpecialModel || (SpecialModel = {}));

    const MISSING_MESH = new Mesh(); ///TODO
    var ItemModel;
    (function (ItemModel) {
        function fromJson(obj) {
            const root = Json.readObject(obj) ?? {};
            const type = Json.readString(root.type)?.replace(/^minecraft:/, '');
            switch (type) {
                case 'empty': return new Empty();
                case 'model': return new Model(Identifier.parse(Json.readString(root.model) ?? ''), Json.readArray(root.tints, ItemTint.fromJson) ?? []);
                case 'composite': return new Composite(Json.readArray(root.models, ItemModel.fromJson) ?? []);
                case 'condition': return new Condition(Condition.propertyFromJson(root), ItemModel.fromJson(root.on_true), ItemModel.fromJson(root.on_false));
                case 'select': return new Select(Select.propertyFromJson(root), new Map(Json.readArray(root.cases, e => Json.readObject(e) ?? {})?.flatMap(caseRoot => {
                    const model = ItemModel.fromJson(caseRoot.model);
                    if (Array.isArray(caseRoot.when)) {
                        return caseRoot.when.map(w => [Json.readString(w) ?? '', model]);
                    }
                    else {
                        return [[Json.readString(caseRoot.when) ?? '', model]];
                    }
                })), root.fallback ? ItemModel.fromJson(root.fallback) : undefined);
                case 'range_dispatch': return new RangeDispatch(RangeDispatch.propertyFromJson(root), Json.readNumber(root.scale) ?? 1, Json.readArray(root.entries, entryObj => {
                    const entryRoot = Json.readObject(entryObj) ?? {};
                    return { threshold: Json.readNumber(entryRoot.threshold) ?? 0, model: ItemModel.fromJson(entryRoot.model) };
                }) ?? [], root.fallback ? ItemModel.fromJson(root.fallback) : undefined);
                case 'special': return new Special(SpecialModel.fromJson(root.model), Identifier.parse(Json.readString(root.base) ?? ''));
                case 'bundle/selected_item': return new BundleSelectedItem();
                default:
                    console.warn(`[deepslate]: Unknown item model type '${type}'`);
                    return { getMesh: () => new Mesh() };
            }
        }
        ItemModel.fromJson = fromJson;
        class Empty {
            getMesh(item, resources, context) {
                return new Mesh();
            }
        }
        ItemModel.Empty = Empty;
        class Model {
            modelId;
            tints;
            constructor(modelId, tints) {
                this.modelId = modelId;
                this.tints = tints;
            }
            getMesh(item, resources, context) {
                const model = resources.getBlockModel(this.modelId);
                if (!model) {
                    console.warn(`[deepslate]: Model '${this.modelId}' does not exist`);
                    return new Mesh();
                }
                const tint = (i) => {
                    if (i < this.tints.length) {
                        return this.tints[i].getTint(item, resources, context);
                    }
                    else {
                        return [1, 1, 1];
                    }
                };
                const mesh = model.getMesh(resources, Cull.none(), tint);
                mesh.transform(model.getDisplayTransform(context.display_context ?? 'gui'));
                return mesh;
            }
        }
        ItemModel.Model = Model;
        class Composite {
            models;
            constructor(models) {
                this.models = models;
            }
            getMesh(item, resources, context) {
                const mesh = new Mesh();
                this.models.forEach(model => mesh.merge(model.getMesh(item, resources, context)));
                return mesh;
            }
        }
        ItemModel.Composite = Composite;
        class Condition {
            property;
            onTrue;
            onFalse;
            constructor(property, onTrue, onFalse) {
                this.property = property;
                this.onTrue = onTrue;
                this.onFalse = onFalse;
            }
            getMesh(item, resources, context) {
                return (this.property(item, resources, context) ? this.onTrue : this.onFalse).getMesh(item, resources, context);
            }
            static propertyFromJson(root) {
                const property = Json.readString(root.property)?.replace(/^minecraft:/, '');
                switch (property) {
                    case 'fishing_rod/cast':
                    case 'selected':
                    case 'carried':
                    case 'extended_view':
                        return (item, resources, context) => context[property] ?? false;
                    case 'view_entity':
                        return (item, resources, context) => context.context_entity_is_view_entity ?? false;
                    case 'using_item':
                        return (item, resources, context) => (context.use_duration ?? -1) >= 0;
                    case 'bundle/has_selected_item':
                        return (item, resources, context) => (context['bundle/selected_item'] ?? -1) >= 0;
                    case 'broken': return (item, resources, context) => {
                        const damage = item.getComponent('damage', resources)?.getAsNumber();
                        const max_damage = item.getComponent('max_damage', resources)?.getAsNumber();
                        return (damage !== undefined && max_damage !== undefined && damage >= max_damage - 1);
                    };
                    case 'damaged': return (item, resources, context) => {
                        const damage = item.getComponent('damage', resources)?.getAsNumber();
                        const max_damage = item.getComponent('max_damage', resources)?.getAsNumber();
                        return (damage !== undefined && max_damage !== undefined && damage >= 1);
                    };
                    case 'has_component':
                        const componentId = Identifier.parse(Json.readString(root.component) ?? '');
                        const ignore_default = Json.readBoolean(root.ignore_default) ?? false;
                        return (item, resources, context) => item.hasComponent(componentId, ignore_default ? undefined : resources);
                    case 'keybind_down':
                        const keybind = Json.readString(root.keybind) ?? '';
                        return (item, resources, context) => context.keybind_down?.includes(keybind) ?? false;
                    case 'custom_model_data':
                        const index = Json.readInt(root.index) ?? 0;
                        return (item, resources, context) => {
                            const tag = item.getComponent('custom_model_data', resources);
                            if (!tag?.isCompound())
                                return false;
                            const flag = tag.getList('flags').getNumber(index);
                            return flag !== undefined && flag !== 0;
                        };
                    default:
                        console.warn(`[deepslate]: Unknown condition property '${property}'`);
                        return () => false;
                }
            }
        }
        ItemModel.Condition = Condition;
        class Select {
            property;
            cases;
            fallback;
            constructor(property, cases, fallback) {
                this.property = property;
                this.cases = cases;
                this.fallback = fallback;
            }
            getMesh(item, resources, context) {
                const value = this.property(item, resources, context);
                return ((value !== null ? this.cases.get(value) : undefined) ?? this.fallback)?.getMesh(item, resources, context) ?? MISSING_MESH;
            }
            static propertyFromJson(root) {
                const property = Json.readString(root.property)?.replace(/^minecraft:/, '');
                switch (property) {
                    case 'main_hand':
                        return (item, resources, context) => context.main_hand ?? 'right';
                    case 'display_context':
                        return (item, resources, context) => context.display_context ?? 'gui';
                    case 'context_dimension':
                        return (item, resources, context) => context.context_dimension?.toString() ?? null;
                    case 'charge_type':
                        const FIREWORK = Identifier.create('firework_rocket');
                        return (item, resources, context) => {
                            const tag = item.getComponent('charged_projectiles', resources);
                            if (!tag?.isList() || tag.length === 0) {
                                return 'none';
                            }
                            return tag.filter(tag => {
                                if (!tag.isCompound()) {
                                    return false;
                                }
                                return Identifier.parse(tag.getString('id')).equals(FIREWORK);
                            }).length > 0 ? 'rocket' : 'arrow';
                        };
                    case 'trim_material':
                        return (item, resources, context) => {
                            const tag = item.getComponent('trim', resources);
                            if (!tag?.isCompound()) {
                                return null;
                            }
                            return Identifier.parse(tag.getString('material')).toString();
                        };
                    case 'block_state':
                        const block_state_property = Json.readString(root.block_state_property) ?? '';
                        return (item, resources, context) => {
                            const tag = item.getComponent('block_state', resources);
                            if (!tag?.isCompound()) {
                                return null;
                            }
                            return tag.getString(block_state_property);
                        };
                    case 'local_time': return (item, resources, context) => 'NOT IMPLEMENTED';
                    case 'context_entity_type':
                        return (item, resources, context) => context.context_entity_type?.toString() ?? null;
                    case 'custom_model_data':
                        const index = Json.readInt(root.index) ?? 0;
                        return (item, resources, context) => {
                            const tag = item.getComponent('custom_model_data', resources);
                            if (!tag?.isCompound()) {
                                return null;
                            }
                            const list = tag.getList('strings');
                            if (list.length <= index) {
                                return null;
                            }
                            return list.getString(index);
                        };
                    default:
                        console.warn(`[deepslate]: Unknown select property '${property}'`);
                        return () => null;
                }
            }
        }
        ItemModel.Select = Select;
        class RangeDispatch {
            property;
            scale;
            fallback;
            entries;
            constructor(property, scale, entries, fallback) {
                this.property = property;
                this.scale = scale;
                this.fallback = fallback;
                this.entries = entries.sort((a, b) => a.threshold - b.threshold);
            }
            getMesh(item, resources, context) {
                const value = this.property(item, resources, context) * this.scale;
                let model = this.fallback;
                for (const entry of this.entries) {
                    if (entry.threshold <= value) {
                        model = entry.model;
                    }
                    else {
                        break;
                    }
                }
                return model?.getMesh(item, resources, context) ?? MISSING_MESH;
            }
            static propertyFromJson(root) {
                const property = Json.readString(root.property)?.replace(/^minecraft:/, '');
                switch (property) {
                    case 'bundle/fullness':
                        function calculateBundleWeight(item, resources) {
                            const tag = item.getComponent('bundle_contents', resources);
                            if (!tag?.isListOrArray()) {
                                return 0;
                            }
                            const items = tag.map(t => t.isCompound() ? ItemStack.fromNbt(t) : undefined);
                            return items.reduce((weight, item) => {
                                if (item === undefined) {
                                    return weight;
                                }
                                if (item.hasComponent('bundle_contents', resources)) {
                                    return weight + calculateBundleWeight(item, resources) + 1 / 16;
                                }
                                const beesTag = item.getComponent('bees', resources);
                                if (beesTag?.isListOrArray() && beesTag.length > 0) {
                                    return weight + 1;
                                }
                                const maxStackSize = item.getComponent('max_stack_size', resources)?.getAsNumber() ?? 1;
                                return weight + item.count / maxStackSize;
                            }, 0);
                        }
                        return (item, resources, context) => calculateBundleWeight(item, resources);
                    case 'damage': {
                        const normalize = Json.readBoolean(root.normalize) ?? true;
                        return (item, resources, context) => {
                            const maxDamage = item.getComponent('max_damage', resources)?.getAsNumber() ?? 0;
                            const damage = clamp$1(item.getComponent('damage', resources)?.getAsNumber() ?? 0, 0, maxDamage);
                            if (normalize)
                                return clamp$1(damage / maxDamage, 0, 1);
                            return clamp$1(damage, 0, maxDamage);
                        };
                    }
                    case 'count': {
                        const normalize = Json.readBoolean(root.normalize) ?? true;
                        return (item, resources, context) => {
                            const maxStackSize = item.getComponent('max_stack_size', resources)?.getAsNumber() ?? 1;
                            if (normalize)
                                return clamp$1(item.count / maxStackSize, 0, 1);
                            return clamp$1(item.count, 0, maxStackSize);
                        };
                    }
                    case 'cooldown': return (item, resources, context) => {
                        const tag = item.getComponent('use_cooldown', resources);
                        const cooldownGroup = tag?.isCompound()
                            ? Identifier.parse(tag.getString('cooldown_group') ?? item.id)
                            : item.id;
                        return context.cooldown_percentage?.[cooldownGroup.toString()] ?? 0;
                    };
                    case 'time':
                        const source = Json.readString(root.source) ?? 'daytime';
                        switch (source) {
                            case 'moon_phase': return (item, resources, context) => ((context.game_time ?? 0) / 24000 % 8) / 8;
                            case 'random': return (item, resources, context) => Math.random();
                            default: return (item, resources, context) => {
                                const gameTime = context.game_time ?? 0;
                                const linearTime = ((gameTime / 24000.0) % 1) - 0.25;
                                const cosTime = 0.5 - Math.cos(linearTime * Math.PI) / 2.0;
                                return (linearTime * 2.0 + cosTime) / 3;
                            };
                        }
                    case 'compass': return (item, resources, context) => context.compass_angle ?? 0; // TODO: calculate properly?
                    case 'crossbow/pull': return (item, resources, context) => context['crossbow/pull'] ?? 0;
                    case 'use_duration':
                        const remaining = Json.readBoolean(root.remaining) ?? true;
                        return (item, resources, context) => {
                            if (context.use_duration === undefined || context.use_duration < 0)
                                return 0;
                            if (remaining)
                                return Math.max((context.max_use_duration ?? 0) - (context.use_duration), 0);
                            return context.use_duration;
                        };
                    case 'use_cycle':
                        const period = Json.readNumber(root.period) ?? 1;
                        return (item, resources, context) => {
                            if (context.use_duration === undefined || context.use_duration < 0)
                                return 0;
                            return Math.max((context.max_use_duration ?? 0) - (context.use_duration ?? 0), 0) % period;
                        };
                    case 'custom_model_data':
                        const index = Json.readInt(root.index) ?? 0;
                        return (item, resources, context) => {
                            const tag = item.getComponent('custom_model_data', resources);
                            if (!tag?.isCompound()) {
                                return 0;
                            }
                            return tag.getList('floats').getNumber(index);
                        };
                    default:
                        console.warn(`[deepslate]: Unknown range dispatch property '${property}'`);
                        return () => 0;
                }
            }
        }
        ItemModel.RangeDispatch = RangeDispatch;
        class Special {
            specialModel;
            base;
            constructor(specialModel, base) {
                this.specialModel = specialModel;
                this.base = base;
            }
            getMesh(item, resources, context) {
                const mesh = this.specialModel.getMesh(item, resources);
                const model = resources.getBlockModel(this.base);
                if (!model) {
                    console.warn(`[deepslate]: Special model base '${this.base}' does not exist`);
                    return new Mesh();
                }
                mesh.transform(model.getDisplayTransform(context.display_context ?? 'gui'));
                return mesh;
            }
        }
        ItemModel.Special = Special;
        class BundleSelectedItem {
            getMesh(item, resources, context) {
                const selectedItemIndex = context['bundle/selected_item'];
                if (selectedItemIndex === undefined || selectedItemIndex < 0)
                    return new Mesh();
                const tag = item.getComponent('bundle_contents', resources);
                if (!tag?.isListOrArray()) {
                    return new Mesh();
                }
                const selectedItemTag = tag.get(selectedItemIndex);
                if (selectedItemTag === undefined || !selectedItemTag.isCompound()) {
                    return new Mesh();
                }
                const selectedItem = ItemStack.fromNbt(selectedItemTag);
                return ItemRenderer.getItemMesh(selectedItem, resources, {
                    ...context,
                    'bundle/selected_item': -1,
                    selected: false,
                    carried: false,
                    use_duration: -1,
                });
            }
        }
        ItemModel.BundleSelectedItem = BundleSelectedItem;
    })(ItemModel || (ItemModel = {}));

    const vsColor = `
  attribute vec4 vertPos;
  attribute vec3 blockPos;

  uniform mat4 mView;
  uniform mat4 mProj;

  varying highp vec3 vColor;

  void main(void) {
    gl_Position = mProj * mView * vertPos;
    vColor = blockPos / 256.0;
  }
`;
    const fsColor = `
  precision highp float;
  varying highp vec3 vColor;

  void main(void) {
    gl_FragColor = vec4(vColor, 1.0);
  }
`;
    const vsGrid = `
  attribute vec4 vertPos;
  attribute vec3 vertColor;

  uniform mat4 mView;
  uniform mat4 mProj;

  varying highp vec3 vColor;

  void main(void) {
    gl_Position = mProj * mView * vertPos;
    vColor = vertColor;
  }
`;
    const fsGrid = `
  precision highp float;
  varying highp vec3 vColor;

  void main(void) {
    gl_FragColor = vec4(vColor, 1.0);
  }
`;
    class StructureRenderer extends Renderer {
        structure;
        resources;
        gridShaderProgram;
        colorShaderProgram;
        gridMesh = new Mesh();
        outlineMesh = new Mesh();
        invisibleBlocksMesh = new Mesh();
        atlasTexture;
        useInvisibleBlocks;
        chunkBuilder;
        constructor(gl, structure, resources, options) {
            super(gl);
            this.structure = structure;
            this.resources = resources;
            const chunkSize = options?.chunkSize ?? 16;
            this.chunkBuilder = new ChunkBuilder(gl, structure, resources, chunkSize);
            if (options?.facesPerBuffer) {
                console.warn('[deepslate renderer warning]: facesPerBuffer option has been removed in favor of chunkSize');
            }
            this.useInvisibleBlocks = options?.useInvisibleBlockBuffer ?? true;
            this.gridShaderProgram = new ShaderProgram(gl, vsGrid, fsGrid).getProgram();
            this.colorShaderProgram = new ShaderProgram(gl, vsColor, fsColor).getProgram();
            this.gridMesh = this.getGridMesh();
            this.outlineMesh = this.getOutlineMesh();
            this.invisibleBlocksMesh = this.getInvisibleBlocksMesh();
            this.atlasTexture = this.createAtlasTexture(this.resources.getTextureAtlas());
        }
        setStructure(structure) {
            this.structure = structure;
            this.chunkBuilder.setStructure(structure);
            this.gridMesh = this.getGridMesh();
            this.invisibleBlocksMesh = this.getInvisibleBlocksMesh();
        }
        updateStructureBuffers(chunkPositions) {
            this.chunkBuilder.updateStructureBuffers(chunkPositions);
        }
        getGridMesh() {
            const [X, Y, Z] = this.structure.getSize();
            const mesh = new Mesh();
            mesh.addLine(0, 0, 0, X, 0, 0, [1, 0, 0]);
            mesh.addLine(0, 0, 0, 0, 0, Z, [0, 0, 1]);
            const c = [0.8, 0.8, 0.8];
            mesh.addLine(0, 0, 0, 0, Y, 0, c);
            mesh.addLine(X, 0, 0, X, Y, 0, c);
            mesh.addLine(0, 0, Z, 0, Y, Z, c);
            mesh.addLine(X, 0, Z, X, Y, Z, c);
            mesh.addLine(0, Y, 0, 0, Y, Z, c);
            mesh.addLine(X, Y, 0, X, Y, Z, c);
            mesh.addLine(0, Y, 0, X, Y, 0, c);
            mesh.addLine(0, Y, Z, X, Y, Z, c);
            for (let x = 1; x <= X; x += 1)
                mesh.addLine(x, 0, 0, x, 0, Z, c);
            for (let z = 1; z <= Z; z += 1)
                mesh.addLine(0, 0, z, X, 0, z, c);
            return mesh.rebuild(this.gl, { pos: true, color: true });
        }
        getOutlineMesh() {
            return new Mesh()
                .addLineCube(0, 0, 0, 1, 1, 1, [1, 1, 1])
                .rebuild(this.gl, { pos: true, color: true });
        }
        getInvisibleBlocksMesh() {
            const mesh = new Mesh();
            if (!this.useInvisibleBlocks) {
                return mesh;
            }
            const size = this.structure.getSize();
            for (let x = 0; x < size[0]; x += 1) {
                for (let y = 0; y < size[1]; y += 1) {
                    for (let z = 0; z < size[2]; z += 1) {
                        const block = this.structure.getBlock([x, y, z]);
                        if (block === undefined)
                            continue;
                        if (block === null) {
                            mesh.addLineCube(x + 0.4375, y + 0.4375, z + 0.4375, x + 0.5625, y + 0.5625, z + 0.5625, [1, 0.25, 0.25]);
                        }
                        else if (block.state.is(BlockState.AIR)) {
                            mesh.addLineCube(x + 0.375, y + 0.375, z + 0.375, x + 0.625, y + 0.625, z + 0.625, [0.5, 0.5, 1]);
                        }
                        else if (block.state.is(new BlockState('cave_air'))) {
                            mesh.addLineCube(x + 0.375, y + 0.375, z + 0.375, x + 0.625, y + 0.625, z + 0.625, [0.5, 1, 0.5]);
                        }
                    }
                }
            }
            return mesh.rebuild(this.gl, { pos: true, color: true });
        }
        drawGrid(viewMatrix) {
            this.setShader(this.gridShaderProgram);
            this.prepareDraw(viewMatrix);
            this.drawMesh(this.gridMesh, { pos: true, color: true });
        }
        drawInvisibleBlocks(viewMatrix) {
            if (!this.useInvisibleBlocks) {
                return;
            }
            this.setShader(this.gridShaderProgram);
            this.prepareDraw(viewMatrix);
            this.drawMesh(this.invisibleBlocksMesh, { pos: true, color: true });
        }
        drawStructure(viewMatrix) {
            this.setShader(this.shaderProgram);
            this.setTexture(this.atlasTexture, this.resources.getPixelSize?.());
            this.prepareDraw(viewMatrix);
            this.chunkBuilder.getMeshes().forEach(mesh => {
                this.drawMesh(mesh, { pos: true, color: true, texture: true, normal: true });
            });
        }
        drawColoredStructure(viewMatrix) {
            this.setShader(this.colorShaderProgram);
            this.prepareDraw(viewMatrix);
            this.chunkBuilder.getMeshes().forEach(mesh => {
                this.drawMesh(mesh, { pos: true, blockPos: true });
            });
        }
        drawOutline(viewMatrix, pos) {
            this.setShader(this.gridShaderProgram);
            const translatedMatrix = create$2();
            copy$1(translatedMatrix, viewMatrix);
            translate(translatedMatrix, translatedMatrix, pos);
            this.prepareDraw(translatedMatrix);
            this.drawMesh(this.outlineMesh, { pos: true, color: true });
        }
    }

    class TextureAtlas {
        img;
        idMap;
        part;
        constructor(img, idMap) {
            this.img = img;
            this.idMap = idMap;
            if (!isPowerOfTwo(img.width) || !isPowerOfTwo(img.height)) {
                throw new Error(`Expected texture atlas dimensions to be powers of two, got ${img.width}x${img.height}.`);
            }
            this.part = 16 / img.width;
        }
        getTextureAtlas() {
            return this.img;
        }
        getTextureUV(id) {
            return this.idMap[id.toString()] ?? [0, 0, this.part, this.part];
        }
        getPixelSize() {
            return this.part / 16;
        }
        static async fromBlobs(textures) {
            const initialWidth = Math.sqrt(Object.keys(textures).length + 1);
            const width = upperPowerOfTwo(initialWidth);
            const pixelWidth = width * 16;
            const part = 1 / width;
            const canvas = document.createElement('canvas');
            canvas.width = pixelWidth;
            canvas.height = pixelWidth;
            const ctx = canvas.getContext('2d');
            this.drawInvalidTexture(ctx);
            const idMap = {};
            let index = 1;
            await Promise.all(Object.keys(textures).map(async (id) => {
                const u = (index % width);
                const v = Math.floor(index / width);
                index += 1;
                idMap[id] = [part * u, part * v, part * u + part, part * v + part];
                const img = await createImageBitmap(textures[id]);
                ctx.drawImage(img, 0, 0, 16, 16, 16 * u, 16 * v, 16, 16);
            }));
            return new TextureAtlas(ctx.getImageData(0, 0, pixelWidth, pixelWidth), idMap);
        }
        static empty() {
            const canvas = document.createElement('canvas');
            canvas.width = 16;
            canvas.height = 16;
            const ctx = canvas.getContext('2d');
            TextureAtlas.drawInvalidTexture(ctx);
            return new TextureAtlas(ctx.getImageData(0, 0, 16, 16), {});
        }
        static drawInvalidTexture(ctx) {
            ctx.fillStyle = 'black';
            ctx.fillRect(0, 0, 16, 16);
            ctx.fillStyle = 'magenta';
            ctx.fillRect(0, 0, 8, 8);
            ctx.fillRect(8, 8, 8, 8);
        }
    }

    var NoiseRouter;
    (function (NoiseRouter) {
        const fieldParser = (obj) => new DensityFunction.HolderHolder(Holder.parser(WorldgenRegistries.DENSITY_FUNCTION, DensityFunction.fromJson)(obj));
        function fromJson(obj) {
            const root = Json.readObject(obj) ?? {};
            return {
                barrier: fieldParser(root.barrier),
                fluidLevelFloodedness: fieldParser(root.fluid_level_floodedness),
                fluidLevelSpread: fieldParser(root.fluid_level_spread),
                lava: fieldParser(root.lava),
                temperature: fieldParser(root.temperature),
                vegetation: fieldParser(root.vegetation),
                continents: fieldParser(root.continents),
                erosion: fieldParser(root.erosion),
                depth: fieldParser(root.depth),
                ridges: fieldParser(root.ridges),
                preliminarySurfaceLevel: fieldParser(root.preliminary_surface_level),
                finalDensity: fieldParser(root.final_density),
                veinToggle: fieldParser(root.vein_toggle),
                veinRidged: fieldParser(root.vein_ridged),
                veinGap: fieldParser(root.vein_gap),
            };
        }
        NoiseRouter.fromJson = fromJson;
        function create(router) {
            return {
                barrier: DensityFunction.Constant.ZERO,
                fluidLevelFloodedness: DensityFunction.Constant.ZERO,
                fluidLevelSpread: DensityFunction.Constant.ZERO,
                lava: DensityFunction.Constant.ZERO,
                temperature: DensityFunction.Constant.ZERO,
                vegetation: DensityFunction.Constant.ZERO,
                continents: DensityFunction.Constant.ZERO,
                erosion: DensityFunction.Constant.ZERO,
                depth: DensityFunction.Constant.ZERO,
                ridges: DensityFunction.Constant.ZERO,
                preliminarySurfaceLevel: DensityFunction.Constant.ZERO,
                finalDensity: DensityFunction.Constant.ZERO,
                veinToggle: DensityFunction.Constant.ZERO,
                veinRidged: DensityFunction.Constant.ZERO,
                veinGap: DensityFunction.Constant.ZERO,
                ...router,
            };
        }
        NoiseRouter.create = create;
        function mapAll(router, visitor) {
            return {
                barrier: router.barrier.mapAll(visitor),
                fluidLevelFloodedness: router.fluidLevelFloodedness.mapAll(visitor),
                fluidLevelSpread: router.fluidLevelSpread.mapAll(visitor),
                lava: router.lava.mapAll(visitor),
                temperature: router.temperature.mapAll(visitor),
                vegetation: router.vegetation.mapAll(visitor),
                continents: router.continents.mapAll(visitor),
                erosion: router.erosion.mapAll(visitor),
                depth: router.depth.mapAll(visitor),
                ridges: router.ridges.mapAll(visitor),
                preliminarySurfaceLevel: router.preliminarySurfaceLevel.mapAll(visitor),
                finalDensity: router.finalDensity.mapAll(visitor),
                veinToggle: router.veinToggle.mapAll(visitor),
                veinRidged: router.veinRidged.mapAll(visitor),
                veinGap: router.veinGap.mapAll(visitor),
            };
        }
        NoiseRouter.mapAll = mapAll;
        const noiseCache = new Map();
        function instantiate(random, noise) {
            const key = noise.key()?.toString();
            if (!key) {
                throw new Error('Cannot instantiate noise from direct holder');
            }
            const randomKey = random.seedKey();
            const cached = noiseCache.get(key);
            if (cached && cached[0] === randomKey[0] && cached[1] === randomKey[1]) {
                return cached[2];
            }
            const result = new NormalNoise(random.fromHashOf(key), noise.value());
            noiseCache.set(key, [randomKey[0], randomKey[1], result]);
            return result;
        }
        NoiseRouter.instantiate = instantiate;
    })(NoiseRouter || (NoiseRouter = {}));

    var NoiseSettings;
    (function (NoiseSettings) {
        function fromJson(obj) {
            const root = Json.readObject(obj) ?? {};
            return {
                minY: Json.readInt(root.min_y) ?? 0,
                height: Json.readInt(root.height) ?? 256,
                xzSize: Json.readInt(root.size_horizontal) ?? 1,
                ySize: Json.readInt(root.size_vertical) ?? 1,
            };
        }
        NoiseSettings.fromJson = fromJson;
        function create(settings) {
            return {
                minY: 0,
                height: 256,
                xzSize: 1,
                ySize: 1,
                ...settings,
            };
        }
        NoiseSettings.create = create;
        function cellHeight(settings) {
            return settings.ySize << 2;
        }
        NoiseSettings.cellHeight = cellHeight;
        function cellWidth(settings) {
            return settings.xzSize << 2;
        }
        NoiseSettings.cellWidth = cellWidth;
        function cellCountY(settings) {
            return settings.height / cellHeight(settings);
        }
        NoiseSettings.cellCountY = cellCountY;
        function minCellY(settings) {
            return Math.floor(settings.minY / cellHeight(settings));
        }
        NoiseSettings.minCellY = minCellY;
    })(NoiseSettings || (NoiseSettings = {}));
    var NoiseSlideSettings;
    (function (NoiseSlideSettings) {
        function fromJson(obj) {
            const root = Json.readObject(obj) ?? {};
            return {
                target: Json.readNumber(root.target) ?? 0,
                size: Json.readInt(root.size) ?? 0,
                offset: Json.readInt(root.offset) ?? 0,
            };
        }
        NoiseSlideSettings.fromJson = fromJson;
        function apply(slide, density, y) {
            if (slide.size <= 0)
                return density;
            const t = (y - slide.offset) / slide.size;
            return clampedLerp(slide.target, density, t);
        }
        NoiseSlideSettings.apply = apply;
    })(NoiseSlideSettings || (NoiseSlideSettings = {}));

    var VerticalAnchor;
    (function (VerticalAnchor) {
        function fromJson(obj) {
            const root = Json.readObject(obj) ?? {};
            if (root.absolute !== undefined) {
                return absolute(Json.readNumber(root.absolute) ?? 0);
            }
            else if (root.above_bottom !== undefined) {
                return aboveBottom(Json.readNumber(root.above_bottom) ?? 0);
            }
            else if (root.below_top !== undefined) {
                return belowTop(Json.readNumber(root.below_top) ?? 0);
            }
            return () => 0;
        }
        VerticalAnchor.fromJson = fromJson;
        function absolute(value) {
            return () => value;
        }
        VerticalAnchor.absolute = absolute;
        function aboveBottom(value) {
            return context => context.minY + value;
        }
        VerticalAnchor.aboveBottom = aboveBottom;
        function belowTop(value) {
            return context => context.minY + context.height - 1 - value;
        }
        VerticalAnchor.belowTop = belowTop;
    })(VerticalAnchor || (VerticalAnchor = {}));

    class SurfaceSystem {
        rule;
        defaultBlock;
        surfaceNoise;
        surfaceSecondaryNoise;
        random;
        positionalRandoms;
        constructor(rule, defaultBlock, seed) {
            this.rule = rule;
            this.defaultBlock = defaultBlock;
            this.random = XoroshiroRandom.create(seed).forkPositional();
            this.surfaceNoise = NoiseRouter.instantiate(this.random, WorldgenRegistries.SURFACE_NOISE);
            this.surfaceSecondaryNoise = NoiseRouter.instantiate(this.random, WorldgenRegistries.SURFACE_SECONDARY_NOISE);
            this.positionalRandoms = new Map();
        }
        buildSurface(chunk, noiseChunk, worldgenContext, getBiome) {
            const minX = ChunkPos.minBlockX(chunk.pos);
            const minZ = ChunkPos.minBlockZ(chunk.pos);
            const surfaceContext = new SurfaceContext(this, chunk, noiseChunk, worldgenContext, getBiome);
            const ruleWithContext = this.rule(surfaceContext);
            for (let x = 0; x < 16; x += 1) {
                const worldX = minX + x;
                for (let z = 0; z < 1; z += 1) {
                    const worldZ = minZ + z;
                    surfaceContext.updateXZ(worldX, worldZ);
                    let stoneDepthAbove = 0;
                    let waterHeight = Number.MIN_SAFE_INTEGER;
                    let stoneDepthOffset = Number.MAX_SAFE_INTEGER;
                    for (let y = chunk.maxY; y >= chunk.minY; y -= 1) {
                        const worldPos = BlockPos.create(worldX, y, worldZ);
                        const oldState = chunk.getBlockState(worldPos);
                        if (oldState.equals(BlockState.AIR)) {
                            stoneDepthAbove = 0;
                            waterHeight = Number.MIN_SAFE_INTEGER;
                            continue;
                        }
                        if (oldState.isFluid()) {
                            if (waterHeight === Number.MIN_SAFE_INTEGER) {
                                waterHeight = y + 1;
                            }
                            continue;
                        }
                        if (stoneDepthOffset >= y) {
                            stoneDepthOffset = Number.MIN_SAFE_INTEGER;
                            for (let i = y - 1; i >= chunk.minY; i -= 1) {
                                const state = chunk.getBlockState(BlockPos.create(worldX, i, worldZ));
                                if (state.equals(BlockState.AIR) || state.isFluid()) {
                                    stoneDepthOffset = i + 1;
                                    break;
                                }
                            }
                        }
                        stoneDepthAbove += 1;
                        const stoneDepthBelow = y - stoneDepthOffset + 1;
                        if (!oldState.equals(this.defaultBlock)) {
                            continue;
                        }
                        surfaceContext.updateY(stoneDepthAbove, stoneDepthBelow, waterHeight, y);
                        const newState = ruleWithContext(worldX, y, worldZ);
                        if (newState) {
                            chunk.setBlockState(worldPos, newState);
                        }
                    }
                }
            }
        }
        getSurfaceDepth(x, z) {
            const noise = this.surfaceNoise.sample(x, 0, z);
            const offset = this.random.at(x, 0, z).nextDouble() * 0.25;
            return noise * 2.75 + 3 + offset;
        }
        getSurfaceSecondary(x, z) {
            return this.surfaceSecondaryNoise.sample(x, 0, z);
        }
        getRandom(name) {
            return computeIfAbsent(this.positionalRandoms, name, () => {
                return this.random.fromHashOf(name);
            });
        }
    }
    class SurfaceContext {
        system;
        chunk;
        noiseChunk;
        context;
        getBiome;
        blockX = 0;
        blockY = 0;
        blockZ = 0;
        stoneDepthAbove = 0;
        stoneDepthBelow = 0;
        surfaceDepth = 0;
        waterHeight = 0;
        biome = () => '';
        surfaceSecondary = () => 0;
        minSurfaceLevel = () => 0;
        constructor(system, chunk, noiseChunk, context, getBiome) {
            this.system = system;
            this.chunk = chunk;
            this.noiseChunk = noiseChunk;
            this.context = context;
            this.getBiome = getBiome;
        }
        updateXZ(x, z) {
            this.blockX = x;
            this.blockZ = z;
            this.surfaceDepth = this.system.getSurfaceDepth(x, z);
            this.surfaceSecondary = lazy$1(() => this.system.getSurfaceSecondary(x, z));
            this.minSurfaceLevel = lazy$1(() => this.calculateMinSurfaceLevel(x, z));
        }
        updateY(stoneDepthAbove, stoneDepthBelow, waterHeight, y) {
            this.blockY = y;
            this.stoneDepthAbove = stoneDepthAbove;
            this.stoneDepthBelow = stoneDepthBelow;
            this.waterHeight = waterHeight;
            this.biome = lazy$1(() => this.getBiome(BlockPos.create(this.blockX, this.blockY, this.blockZ)));
        }
        calculateMinSurfaceLevel(x, z) {
            const cellX = x >> 4;
            const cellZ = z >> 4;
            const level00 = this.noiseChunk.getPreliminarySurfaceLevel(cellX << 4, cellZ << 4);
            const level10 = this.noiseChunk.getPreliminarySurfaceLevel((cellX + 1) << 4, cellZ << 4);
            const level01 = this.noiseChunk.getPreliminarySurfaceLevel(cellX << 4, (cellZ + 1) << 4);
            const level11 = this.noiseChunk.getPreliminarySurfaceLevel((cellX + 1) << 4, (cellZ + 1) << 4);
            const level = Math.floor(lerp2((x & 0xF) / 16, (z & 0xF) / 16, level00, level10, level01, level11));
            return level + this.surfaceDepth - 8;
        }
    }
    var SurfaceRule;
    (function (SurfaceRule) {
        SurfaceRule.NOOP = () => () => undefined;
        function fromJson(obj) {
            const root = Json.readObject(obj) ?? {};
            const type = Json.readString(root.type)?.replace(/^minecraft:/, '');
            switch (type) {
                case 'block': return block(BlockState.fromJson(root.result_state));
                case 'sequence': return sequence(Json.readArray(root.sequence, SurfaceRule.fromJson) ?? []);
                case 'condition': return condition(SurfaceCondition.fromJson(root.if_true), SurfaceRule.fromJson(root.then_run));
            }
            return SurfaceRule.NOOP;
        }
        SurfaceRule.fromJson = fromJson;
        function block(state) {
            return () => () => state;
        }
        SurfaceRule.block = block;
        function sequence(rules) {
            return context => {
                const rulesWithContext = rules.map(rule => rule(context));
                return (x, y, z) => {
                    for (const rule of rulesWithContext) {
                        const result = rule(x, y, z);
                        if (result)
                            return result;
                    }
                    return undefined;
                };
            };
        }
        SurfaceRule.sequence = sequence;
        function condition(ifTrue, thenRun) {
            return context => (x, y, z) => {
                if (ifTrue(context)) {
                    return thenRun(context)(x, y, z);
                }
                return undefined;
            };
        }
        SurfaceRule.condition = condition;
    })(SurfaceRule || (SurfaceRule = {}));
    var SurfaceCondition;
    (function (SurfaceCondition) {
        SurfaceCondition.FALSE = () => false;
        SurfaceCondition.TRUE = () => true;
        function fromJson(obj) {
            const root = Json.readObject(obj) ?? {};
            const type = Json.readString(root.type)?.replace(/^minecraft:/, '');
            switch (type) {
                case 'above_preliminary_surface': return abovePreliminarySurface();
                case 'biome': return biome(Json.readArray(root.biome_is, e => Json.readString(e) ?? '') ?? []);
                case 'not': return not(SurfaceCondition.fromJson(root.invert));
                case 'stone_depth': return stoneDepth(Json.readInt(root.offset) ?? 0, Json.readBoolean(root.add_surface_depth) ?? false, Json.readInt(root.secondary_depth_range) ?? 0, Json.readString(root.surface_type) === 'ceiling');
                case 'vertical_gradient': return verticalGradient(Json.readString(root.random_name) ?? '', VerticalAnchor.fromJson(root.true_at_and_below), VerticalAnchor.fromJson(root.false_at_and_above));
                case 'water': return water(Json.readInt(root.offset) ?? 0, Json.readInt(root.surface_depth_multiplier) ?? 0, Json.readBoolean(root.add_surface_depth) ?? false);
                case 'y_above': return yAbove(VerticalAnchor.fromJson(root.anchor), Json.readInt(root.surface_depth_multiplier) ?? 0, Json.readBoolean(root.add_surface_depth) ?? false);
            }
            return SurfaceCondition.FALSE;
        }
        SurfaceCondition.fromJson = fromJson;
        function abovePreliminarySurface() {
            return context => context.blockY >= context.minSurfaceLevel();
        }
        SurfaceCondition.abovePreliminarySurface = abovePreliminarySurface;
        function biome(biomes) {
            const biomeSet = new Set(biomes);
            return context => biomeSet.has(context.biome());
        }
        SurfaceCondition.biome = biome;
        function not(invert) {
            return context => !invert(context);
        }
        SurfaceCondition.not = not;
        function stoneDepth(offset, addSurfaceDepth, secondaryDepthRange, ceiling) {
            return context => {
                const depth = ceiling ? context.stoneDepthBelow : context.stoneDepthAbove;
                const surfaceDepth = addSurfaceDepth ? context.surfaceDepth : 0;
                const secondaryDepth = secondaryDepthRange === 0 ? 0 : map(context.surfaceSecondary(), -1, 1, 0, secondaryDepthRange);
                return depth <= 1 + offset + surfaceDepth + secondaryDepth;
            };
        }
        SurfaceCondition.stoneDepth = stoneDepth;
        function verticalGradient(randomName, trueAtAndBelow, falseAtAndAbove) {
            return context => {
                const trueAtAndBelowY = trueAtAndBelow(context.context);
                const falseAtAndAboveY = falseAtAndAbove(context.context);
                if (context.blockY <= trueAtAndBelowY) {
                    return true;
                }
                if (context.blockY >= falseAtAndAboveY) {
                    return false;
                }
                const random = context.system.getRandom(randomName);
                const chance = map(context.blockY, trueAtAndBelowY, falseAtAndAboveY, 1, 0);
                return random.nextFloat() < chance;
            };
        }
        SurfaceCondition.verticalGradient = verticalGradient;
        function water(offset, surfaceDepthMultiplier, addStoneDepth) {
            return context => {
                if (context.waterHeight === Number.MIN_SAFE_INTEGER) {
                    return true;
                }
                const stoneDepth = addStoneDepth ? context.stoneDepthAbove : 0;
                return context.blockY + stoneDepth >= context.waterHeight + offset + context.surfaceDepth * surfaceDepthMultiplier;
            };
        }
        SurfaceCondition.water = water;
        function yAbove(anchor, surfaceDepthMultiplier, addStoneDepth) {
            return context => {
                const stoneDepth = addStoneDepth ? context.stoneDepthAbove : 0;
                return context.blockY + stoneDepth >= anchor(context.context) + context.surfaceDepth * surfaceDepthMultiplier;
            };
        }
        SurfaceCondition.yAbove = yAbove;
    })(SurfaceCondition || (SurfaceCondition = {}));

    var NoiseGeneratorSettings;
    (function (NoiseGeneratorSettings) {
        function fromJson(obj) {
            const root = Json.readObject(obj) ?? {};
            return {
                surfaceRule: SurfaceRule.fromJson(root.surface_rule),
                noise: NoiseSettings.fromJson(root.noise),
                defaultBlock: BlockState.fromJson(root.default_block),
                defaultFluid: BlockState.fromJson(root.default_fluid),
                noiseRouter: NoiseRouter.fromJson(root.noise_router),
                seaLevel: Json.readInt(root.sea_level) ?? 0,
                disableMobGeneration: Json.readBoolean(root.disable_mob_generation) ?? false,
                aquifersEnabled: Json.readBoolean(root.aquifers_enabled) ?? false,
                oreVeinsEnabled: Json.readBoolean(root.ore_veins_enabled) ?? false,
                legacyRandomSource: Json.readBoolean(root.legacy_random_source) ?? false,
            };
        }
        NoiseGeneratorSettings.fromJson = fromJson;
        function create(settings) {
            return {
                surfaceRule: SurfaceRule.NOOP,
                noise: NoiseSettings.create({}),
                defaultBlock: BlockState.STONE,
                defaultFluid: BlockState.WATER,
                noiseRouter: NoiseRouter.create({}),
                seaLevel: 0,
                disableMobGeneration: false,
                aquifersEnabled: false,
                oreVeinsEnabled: false,
                legacyRandomSource: false,
                ...settings,
            };
        }
        NoiseGeneratorSettings.create = create;
    })(NoiseGeneratorSettings || (NoiseGeneratorSettings = {}));

    var WorldgenRegistries;
    (function (WorldgenRegistries) {
        WorldgenRegistries.NOISE = Registry.createAndRegister('worldgen/noise', NoiseParameters.fromJson);
        WorldgenRegistries.DENSITY_FUNCTION = Registry.createAndRegister('worldgen/density_function', obj => DensityFunction.fromJson(obj));
        WorldgenRegistries.NOISE_SETTINGS = Registry.createAndRegister('worldgen/noise_settings', NoiseGeneratorSettings.fromJson);
        WorldgenRegistries.BIOME = Registry.createAndRegister('worldgen/biome');
        WorldgenRegistries.SURFACE_NOISE = createNoise('surface', -6, [1, 1, 1]);
        WorldgenRegistries.SURFACE_SECONDARY_NOISE = createNoise('surface_secondary', -6, [1, 1, 0, 1]);
        function createNoise(name, firstOctave, amplitudes) {
            return WorldgenRegistries.NOISE.register(Identifier.create(name), NoiseParameters.create(firstOctave, amplitudes), true);
        }
    })(WorldgenRegistries || (WorldgenRegistries = {}));

    class DensityFunction {
        minValue() {
            return -this.maxValue();
        }
        mapAll(visitor) {
            return visitor.map(this);
        }
    }
    (function (DensityFunction) {
        function context(x, y, z) {
            return {
                x,
                y,
                z,
            };
        }
        DensityFunction.context = context;
        class Transformer extends DensityFunction {
            input;
            constructor(input) {
                super();
                this.input = input;
            }
            compute(context) {
                return this.transform(context, this.input.compute(context));
            }
        }
        const NoiseParser = Holder.parser(WorldgenRegistries.NOISE, NoiseParameters.fromJson);
        function fromJson(obj, inputParser = fromJson) {
            if (typeof obj === 'string') {
                return new HolderHolder(Holder.reference(WorldgenRegistries.DENSITY_FUNCTION, Identifier.parse(obj)));
            }
            if (typeof obj === 'number') {
                return new Constant(obj);
            }
            const root = Json.readObject(obj) ?? {};
            const type = Json.readString(root.type)?.replace(/^minecraft:/, '');
            switch (type) {
                case 'blend_alpha': return new ConstantMinMax(1, 0, 1);
                case 'blend_offset': return new ConstantMinMax(0, -Infinity, Infinity);
                case 'beardifier': return new ConstantMinMax(0, -Infinity, Infinity);
                case 'old_blended_noise': return new OldBlendedNoise(Json.readNumber(root.xz_scale) ?? 1, Json.readNumber(root.y_scale) ?? 1, Json.readNumber(root.xz_factor) ?? 80, Json.readNumber(root.y_factor) ?? 160, Json.readNumber(root.smear_scale_multiplier) ?? 8);
                case 'flat_cache': return new FlatCache(inputParser(root.argument));
                case 'interpolated': return new Interpolated(inputParser(root.argument));
                case 'cache_2d': return new Cache2D(inputParser(root.argument));
                case 'cache_once': return new CacheOnce(inputParser(root.argument));
                case 'cache_all_in_cell': return new CacheAllInCell(inputParser(root.argument));
                case 'noise': return new Noise(Json.readNumber(root.xz_scale) ?? 1, Json.readNumber(root.y_scale) ?? 1, NoiseParser(root.noise));
                case 'end_islands': return new EndIslands();
                case 'find_top_surface': return new FindTopSurface(inputParser(root.density), inputParser(root.upper_bound), Json.readInt(root.lower_bound) ?? 0, Json.readInt(root.cell_height) ?? 1);
                case 'weird_scaled_sampler': return new WeirdScaledSampler(inputParser(root.input), Json.readEnum(root.rarity_value_mapper, RarityValueMapper), NoiseParser(root.noise));
                case 'shifted_noise': return new ShiftedNoise(inputParser(root.shift_x), inputParser(root.shift_y), inputParser(root.shift_z), Json.readNumber(root.xz_scale) ?? 1, Json.readNumber(root.y_scale) ?? 1, NoiseParser(root.noise));
                case 'range_choice': return new RangeChoice(inputParser(root.input), Json.readNumber(root.min_inclusive) ?? 0, Json.readNumber(root.max_exclusive) ?? 1, inputParser(root.when_in_range), inputParser(root.when_out_of_range));
                case 'shift_a': return new ShiftA(NoiseParser(root.argument));
                case 'shift_b': return new ShiftB(NoiseParser(root.argument));
                case 'shift': return new Shift(NoiseParser(root.argument));
                case 'blend_density': return new BlendDensity(inputParser(root.argument));
                case 'clamp': return new Clamp(inputParser(root.input), Json.readNumber(root.min) ?? 0, Json.readNumber(root.max) ?? 1);
                case 'abs':
                case 'square':
                case 'cube':
                case 'half_negative':
                case 'invert':
                case 'quarter_negative':
                case 'squeeze':
                    return new Mapped(type, inputParser(root.argument));
                case 'add':
                case 'mul':
                case 'min':
                case 'max': return new Ap2(Json.readEnum(type, Ap2Type), inputParser(root.argument1), inputParser(root.argument2));
                case 'spline': return new Spline(CubicSpline.fromJson(root.spline, inputParser));
                case 'constant': return new Constant(Json.readNumber(root.argument) ?? 0);
                case 'y_clamped_gradient': return new YClampedGradient(Json.readInt(root.from_y) ?? -4064, Json.readInt(root.to_y) ?? 4062, Json.readNumber(root.from_value) ?? -4064, Json.readNumber(root.to_value) ?? 4062);
            }
            return Constant.ZERO;
        }
        DensityFunction.fromJson = fromJson;
        class Constant extends DensityFunction {
            value;
            static ZERO = new Constant(0);
            static ONE = new Constant(1);
            constructor(value) {
                super();
                this.value = value;
            }
            compute() {
                return this.value;
            }
            minValue() {
                return this.value;
            }
            maxValue() {
                return this.value;
            }
        }
        DensityFunction.Constant = Constant;
        class HolderHolder extends DensityFunction {
            holder;
            constructor(holder) {
                super();
                this.holder = holder;
            }
            compute(context) {
                return this.holder.value().compute(context);
            }
            minValue() {
                return this.holder.value().minValue();
            }
            maxValue() {
                return this.holder.value().maxValue();
            }
        }
        DensityFunction.HolderHolder = HolderHolder;
        class ConstantMinMax extends DensityFunction.Constant {
            min;
            max;
            constructor(value, min, max) {
                super(value);
                this.min = min;
                this.max = max;
            }
            minValue() {
                return this.min;
            }
            maxValue() {
                return this.max;
            }
        }
        DensityFunction.ConstantMinMax = ConstantMinMax;
        class OldBlendedNoise extends DensityFunction {
            xzScale;
            yScale;
            xzFactor;
            yFactor;
            smearScaleMultiplier;
            blendedNoise;
            constructor(xzScale, yScale, xzFactor, yFactor, smearScaleMultiplier, blendedNoise) {
                super();
                this.xzScale = xzScale;
                this.yScale = yScale;
                this.xzFactor = xzFactor;
                this.yFactor = yFactor;
                this.smearScaleMultiplier = smearScaleMultiplier;
                this.blendedNoise = blendedNoise;
            }
            compute(context) {
                return this.blendedNoise?.sample(context.x, context.y, context.z) ?? 0;
            }
            maxValue() {
                return this.blendedNoise?.maxValue ?? 0;
            }
        }
        DensityFunction.OldBlendedNoise = OldBlendedNoise;
        class Wrapper extends DensityFunction {
            wrapped;
            constructor(wrapped) {
                super();
                this.wrapped = wrapped;
            }
            minValue() {
                return this.wrapped.minValue();
            }
            maxValue() {
                return this.wrapped.maxValue();
            }
        }
        class FlatCache extends Wrapper {
            lastQuartX;
            lastQuartZ;
            lastValue = 0;
            constructor(wrapped) {
                super(wrapped);
            }
            compute(context) {
                const quartX = context.x >> 2;
                const quartZ = context.z >> 2;
                if (this.lastQuartX !== quartX || this.lastQuartZ !== quartZ) {
                    this.lastValue = this.wrapped.compute(DensityFunction.context(quartX << 2, 0, quartZ << 2));
                    this.lastQuartX = quartX;
                    this.lastQuartZ = quartZ;
                }
                return this.lastValue;
            }
            mapAll(visitor) {
                return visitor.map(new FlatCache(this.wrapped.mapAll(visitor)));
            }
        }
        DensityFunction.FlatCache = FlatCache;
        class CacheAllInCell extends Wrapper {
            constructor(wrapped) {
                super(wrapped);
            }
            compute(context) {
                return this.wrapped.compute(context);
            }
            mapAll(visitor) {
                return visitor.map(new CacheAllInCell(this.wrapped.mapAll(visitor)));
            }
        }
        DensityFunction.CacheAllInCell = CacheAllInCell;
        class Cache2D extends Wrapper {
            lastBlockX;
            lastBlockZ;
            lastValue = 0;
            constructor(wrapped) {
                super(wrapped);
            }
            compute(context) {
                const blockX = context.x;
                const blockZ = context.z;
                if (this.lastBlockX !== blockX || this.lastBlockZ !== blockZ) {
                    this.lastValue = this.wrapped.compute(context);
                    this.lastBlockX = blockX;
                    this.lastBlockZ = blockZ;
                }
                return this.lastValue;
            }
            mapAll(visitor) {
                return visitor.map(new Cache2D(this.wrapped.mapAll(visitor)));
            }
        }
        DensityFunction.Cache2D = Cache2D;
        class CacheOnce extends Wrapper {
            lastBlockX;
            lastBlockY;
            lastBlockZ;
            lastValue = 0;
            constructor(wrapped) {
                super(wrapped);
            }
            compute(context) {
                const blockX = context.x;
                const blockY = context.y;
                const blockZ = context.z;
                if (this.lastBlockX !== blockX || this.lastBlockY !== blockY || this.lastBlockZ !== blockZ) {
                    this.lastValue = this.wrapped.compute(context);
                    this.lastBlockX = blockX;
                    this.lastBlockY = blockY;
                    this.lastBlockZ = blockZ;
                }
                return this.lastValue;
            }
            mapAll(visitor) {
                return visitor.map(new CacheOnce(this.wrapped.mapAll(visitor)));
            }
        }
        DensityFunction.CacheOnce = CacheOnce;
        class Interpolated extends Wrapper {
            cellWidth;
            cellHeight;
            values;
            constructor(wrapped, cellWidth = 4, cellHeight = 4) {
                super(wrapped);
                this.cellWidth = cellWidth;
                this.cellHeight = cellHeight;
                this.values = new Map();
            }
            compute({ x: blockX, y: blockY, z: blockZ }) {
                const w = this.cellWidth;
                const h = this.cellHeight;
                const x = ((blockX % w + w) % w) / w;
                const y = ((blockY % h + h) % h) / h;
                const z = ((blockZ % w + w) % w) / w;
                const firstX = Math.floor(blockX / w) * w;
                const firstY = Math.floor(blockY / h) * h;
                const firstZ = Math.floor(blockZ / w) * w;
                const noise000 = () => this.computeCorner(firstX, firstY, firstZ);
                const noise001 = () => this.computeCorner(firstX, firstY, firstZ + w);
                const noise010 = () => this.computeCorner(firstX, firstY + h, firstZ);
                const noise011 = () => this.computeCorner(firstX, firstY + h, firstZ + w);
                const noise100 = () => this.computeCorner(firstX + w, firstY, firstZ);
                const noise101 = () => this.computeCorner(firstX + w, firstY, firstZ + w);
                const noise110 = () => this.computeCorner(firstX + w, firstY + h, firstZ);
                const noise111 = () => this.computeCorner(firstX + w, firstY + h, firstZ + w);
                return lazyLerp3(x, y, z, noise000, noise100, noise010, noise110, noise001, noise101, noise011, noise111);
            }
            computeCorner(x, y, z) {
                return computeIfAbsent(this.values, `${x} ${y} ${z}`, () => {
                    return this.wrapped.compute(DensityFunction.context(x, y, z));
                });
            }
            mapAll(visitor) {
                return visitor.map(new Interpolated(this.wrapped.mapAll(visitor)));
            }
            withCellSize(cellWidth, cellHeight) {
                return new Interpolated(this.wrapped, cellWidth, cellHeight);
            }
        }
        DensityFunction.Interpolated = Interpolated;
        class Noise extends DensityFunction {
            xzScale;
            yScale;
            noiseData;
            noise;
            constructor(xzScale, yScale, noiseData, noise) {
                super();
                this.xzScale = xzScale;
                this.yScale = yScale;
                this.noiseData = noiseData;
                this.noise = noise;
            }
            compute(context) {
                return this.noise?.sample(context.x * this.xzScale, context.y * this.yScale, context.z * this.xzScale) ?? 0;
            }
            maxValue() {
                return this.noise?.maxValue ?? 2;
            }
        }
        DensityFunction.Noise = Noise;
        class EndIslands extends DensityFunction {
            islandNoise;
            constructor(seed) {
                super();
                const random = new LegacyRandom(seed ?? BigInt(0));
                random.consume(17292);
                this.islandNoise = new SimplexNoise(random);
            }
            getHeightValue(x, z) {
                const x0 = Math.floor(x / 2);
                const z0 = Math.floor(z / 2);
                const x1 = x % 2;
                const z1 = z % 2;
                let f = clamp$1(100 - Math.sqrt(x * x + z * z) * 8, -100, 80);
                for (let i = -12; i <= 12; i += 1) {
                    for (let j = -12; j <= 12; j += 1) {
                        const x2 = x0 + i;
                        const z2 = z0 + j;
                        if (x2 * x2 + z2 * z2 <= 4096 || this.islandNoise.sample2D(x2, z2) >= -0.9) {
                            continue;
                        }
                        const f1 = (Math.abs(x2) * 3439 + Math.abs(z2) * 147) % 13 + 9;
                        const x3 = x1 + i * 2;
                        const z3 = z1 + j * 2;
                        const f2 = 100 - Math.sqrt(x3 * x3 + z3 * z3) * f1;
                        const f3 = clamp$1(f2, -100, 80);
                        f = Math.max(f, f3);
                    }
                }
                return f;
            }
            compute({ x, y, z }) {
                return (this.getHeightValue(Math.floor(x / 8), Math.floor(z / 8)) - 8) / 128;
            }
            minValue() {
                return -0.84375;
            }
            maxValue() {
                return 0.5625;
            }
        }
        DensityFunction.EndIslands = EndIslands;
        class FindTopSurface extends DensityFunction {
            density;
            upperBound;
            lowerBound;
            cellHeight;
            constructor(density, upperBound, lowerBound, cellHeight) {
                super();
                this.density = density;
                this.upperBound = upperBound;
                this.lowerBound = lowerBound;
                this.cellHeight = cellHeight;
            }
            compute(context) {
                const topY = Math.floor(this.upperBound.compute(context) / this.cellHeight) * this.cellHeight;
                if (topY < this.lowerBound) {
                    return this.lowerBound;
                }
                for (let blockY = topY; blockY >= this.lowerBound; blockY -= this.cellHeight) {
                    if (this.density.compute(DensityFunction.context(context.x, blockY, context.z)) > 0) {
                        return blockY;
                    }
                }
                return this.lowerBound;
            }
            mapAll(visitor) {
                return visitor.map(new FindTopSurface(this.density.mapAll(visitor), this.upperBound.mapAll(visitor), this.lowerBound, this.cellHeight));
            }
            minValue() {
                return this.lowerBound;
            }
            maxValue() {
                return Math.max(this.lowerBound, this.upperBound.maxValue());
            }
        }
        DensityFunction.FindTopSurface = FindTopSurface;
        const RarityValueMapper = ['type_1', 'type_2'];
        class WeirdScaledSampler extends Transformer {
            rarityValueMapper;
            noiseData;
            noise;
            static ValueMapper = {
                type_1: WeirdScaledSampler.rarityValueMapper1,
                type_2: WeirdScaledSampler.rarityValueMapper2,
            };
            mapper;
            constructor(input, rarityValueMapper, noiseData, noise) {
                super(input);
                this.rarityValueMapper = rarityValueMapper;
                this.noiseData = noiseData;
                this.noise = noise;
                this.mapper = WeirdScaledSampler.ValueMapper[this.rarityValueMapper];
            }
            transform(context, density) {
                if (!this.noise) {
                    return 0;
                }
                const rarity = this.mapper(density);
                return rarity * Math.abs(this.noise.sample(context.x / rarity, context.y / rarity, context.z / rarity));
            }
            mapAll(visitor) {
                return visitor.map(new WeirdScaledSampler(this.input.mapAll(visitor), this.rarityValueMapper, this.noiseData, this.noise));
            }
            minValue() {
                return 0;
            }
            maxValue() {
                return this.rarityValueMapper === 'type_1' ? 2 : 3;
            }
            static rarityValueMapper1(value) {
                if (value < -0.5) {
                    return 0.75;
                }
                else if (value < 0) {
                    return 1;
                }
                else if (value < 0.5) {
                    return 1.5;
                }
                else {
                    return 2;
                }
            }
            static rarityValueMapper2(value) {
                if (value < -0.75) {
                    return 0.5;
                }
                else if (value < -0.5) {
                    return 0.75;
                }
                else if (value < 0.5) {
                    return 1;
                }
                else if (value < 0.75) {
                    return 2;
                }
                else {
                    return 3;
                }
            }
        }
        DensityFunction.WeirdScaledSampler = WeirdScaledSampler;
        class ShiftedNoise extends Noise {
            shiftX;
            shiftY;
            shiftZ;
            constructor(shiftX, shiftY, shiftZ, xzScale, yScale, noiseData, noise) {
                super(xzScale, yScale, noiseData, noise);
                this.shiftX = shiftX;
                this.shiftY = shiftY;
                this.shiftZ = shiftZ;
            }
            compute(context) {
                const xx = context.x * this.xzScale + this.shiftX.compute(context);
                const yy = context.y * this.yScale + this.shiftY.compute(context);
                const zz = context.z * this.xzScale + this.shiftZ.compute(context);
                return this.noise?.sample(xx, yy, zz) ?? 0;
            }
            mapAll(visitor) {
                return visitor.map(new ShiftedNoise(this.shiftX.mapAll(visitor), this.shiftY.mapAll(visitor), this.shiftZ.mapAll(visitor), this.xzScale, this.yScale, this.noiseData, this.noise));
            }
        }
        DensityFunction.ShiftedNoise = ShiftedNoise;
        class RangeChoice extends DensityFunction {
            input;
            minInclusive;
            maxExclusive;
            whenInRange;
            whenOutOfRange;
            constructor(input, minInclusive, maxExclusive, whenInRange, whenOutOfRange) {
                super();
                this.input = input;
                this.minInclusive = minInclusive;
                this.maxExclusive = maxExclusive;
                this.whenInRange = whenInRange;
                this.whenOutOfRange = whenOutOfRange;
            }
            compute(context) {
                const x = this.input.compute(context);
                return (this.minInclusive <= x && x < this.maxExclusive)
                    ? this.whenInRange.compute(context)
                    : this.whenOutOfRange.compute(context);
            }
            mapAll(visitor) {
                return visitor.map(new RangeChoice(this.input.mapAll(visitor), this.minInclusive, this.maxExclusive, this.whenInRange.mapAll(visitor), this.whenOutOfRange.mapAll(visitor)));
            }
            minValue() {
                return Math.min(this.whenInRange.minValue(), this.whenOutOfRange.minValue());
            }
            maxValue() {
                return Math.max(this.whenInRange.maxValue(), this.whenOutOfRange.maxValue());
            }
        }
        DensityFunction.RangeChoice = RangeChoice;
        class ShiftNoise extends DensityFunction {
            noiseData;
            offsetNoise;
            constructor(noiseData, offsetNoise) {
                super();
                this.noiseData = noiseData;
                this.offsetNoise = offsetNoise;
            }
            compute(context) {
                return (this.offsetNoise?.sample(context.x * 0.25, context.y * 0.25, context.z * 0.25) ?? 0) * 4;
            }
            maxValue() {
                return (this.offsetNoise?.maxValue ?? 2) * 4;
            }
        }
        DensityFunction.ShiftNoise = ShiftNoise;
        class ShiftA extends ShiftNoise {
            constructor(noiseData, offsetNoise) {
                super(noiseData, offsetNoise);
            }
            compute(context) {
                return super.compute(DensityFunction.context(context.x, 0, context.z));
            }
            withNewNoise(newNoise) {
                return new ShiftA(this.noiseData, newNoise);
            }
        }
        DensityFunction.ShiftA = ShiftA;
        class ShiftB extends ShiftNoise {
            constructor(noiseData, offsetNoise) {
                super(noiseData, offsetNoise);
            }
            compute(context) {
                return super.compute(DensityFunction.context(context.z, context.x, 0));
            }
            withNewNoise(newNoise) {
                return new ShiftB(this.noiseData, newNoise);
            }
        }
        DensityFunction.ShiftB = ShiftB;
        class Shift extends ShiftNoise {
            constructor(noiseData, offsetNoise) {
                super(noiseData, offsetNoise);
            }
            withNewNoise(newNoise) {
                return new Shift(this.noiseData, newNoise);
            }
        }
        DensityFunction.Shift = Shift;
        class BlendDensity extends Transformer {
            constructor(input) {
                super(input);
            }
            transform(context, density) {
                return density; // blender not supported
            }
            mapAll(visitor) {
                return visitor.map(new BlendDensity(this.input.mapAll(visitor)));
            }
            minValue() {
                return -Infinity;
            }
            maxValue() {
                return Infinity;
            }
        }
        DensityFunction.BlendDensity = BlendDensity;
        class Clamp extends Transformer {
            min;
            max;
            constructor(input, min, max) {
                super(input);
                this.min = min;
                this.max = max;
            }
            transform(context, density) {
                return clamp$1(density, this.min, this.max);
            }
            mapAll(visitor) {
                return visitor.map(new Clamp(this.input.mapAll(visitor), this.min, this.max));
            }
            minValue() {
                return this.min;
            }
            maxValue() {
                return this.max;
            }
        }
        DensityFunction.Clamp = Clamp;
        class Mapped extends Transformer {
            type;
            min;
            max;
            static MappedTypes = {
                abs: d => Math.abs(d),
                square: d => d * d,
                cube: d => d * d * d,
                half_negative: d => d > 0 ? d : d * 0.5,
                invert: d => 1 / d,
                quarter_negative: d => d > 0 ? d : d * 0.25,
                squeeze: d => {
                    const c = clamp$1(d, -1, 1);
                    return c / 2 - c * c * c / 24;
                },
            };
            transformer;
            constructor(type, input, min, max) {
                super(input);
                this.type = type;
                this.min = min;
                this.max = max;
                this.transformer = Mapped.MappedTypes[this.type];
            }
            transform(context, density) {
                return this.transformer(density);
            }
            mapAll(visitor) {
                return visitor.map(new Mapped(this.type, this.input.mapAll(visitor)));
            }
            minValue() {
                return this.min ?? -Infinity;
            }
            maxValue() {
                return this.max ?? Infinity;
            }
            withMinMax() {
                const minInput = this.input.minValue();
                let min = this.transformer(minInput);
                let max = this.transformer(this.input.maxValue());
                if (this.type === 'invert') {
                    if (min < 0 && max > 0) {
                        [min, max] = [-Infinity, Infinity];
                    }
                    else {
                        [min, max] = [max, min];
                    }
                }
                else if (this.type === 'abs' || this.type === 'square') {
                    max = Math.max(min, max);
                    min = Math.max(0, minInput);
                }
                return new Mapped(this.type, this.input, min, max);
            }
        }
        DensityFunction.Mapped = Mapped;
        const Ap2Type = ['add', 'mul', 'min', 'max'];
        class Ap2 extends DensityFunction {
            type;
            argument1;
            argument2;
            min;
            max;
            constructor(type, argument1, argument2, min, max) {
                super();
                this.type = type;
                this.argument1 = argument1;
                this.argument2 = argument2;
                this.min = min;
                this.max = max;
            }
            compute(context) {
                const a = this.argument1.compute(context);
                switch (this.type) {
                    case 'add': return a + this.argument2.compute(context);
                    case 'mul': return a === 0 ? 0 : a * this.argument2.compute(context);
                    case 'min': return a < this.argument2.minValue() ? a : Math.min(a, this.argument2.compute(context));
                    case 'max': return a > this.argument2.maxValue() ? a : Math.max(a, this.argument2.compute(context));
                }
            }
            mapAll(visitor) {
                return visitor.map(new Ap2(this.type, this.argument1.mapAll(visitor), this.argument2.mapAll(visitor)));
            }
            minValue() {
                return this.min ?? -Infinity;
            }
            maxValue() {
                return this.max ?? Infinity;
            }
            withMinMax() {
                const min1 = this.argument1.minValue();
                const min2 = this.argument2.minValue();
                const max1 = this.argument1.maxValue();
                const max2 = this.argument2.maxValue();
                if ((this.type === 'min' || this.type === 'max') && (min1 >= max2 || min2 >= max1)) {
                    console.warn(`Creating a ${this.type} function between two non-overlapping inputs`);
                }
                let min, max;
                switch (this.type) {
                    case 'add':
                        min = min1 + min2;
                        max = max1 + max2;
                        break;
                    case 'mul':
                        min = min1 > 0 && min2 > 0 ? (min1 * min2) || 0
                            : max1 < 0 && max2 < 0 ? (max1 * max2) || 0
                                : Math.min((min1 * max2) || 0, (min2 * max1) || 0);
                        max = min1 > 0 && min2 > 0 ? (max1 * max2) || 0
                            : max1 < 0 && max2 < 0 ? (min1 * min2) || 0
                                : Math.max((min1 * min2) || 0, (max1 * max2) || 0);
                        break;
                    case 'min':
                        min = Math.min(min1, min2);
                        max = Math.min(max1, max2);
                        break;
                    case 'max':
                        min = Math.max(min1, min2);
                        max = Math.max(max1, max2);
                        break;
                }
                return new Ap2(this.type, this.argument1, this.argument2, min, max);
            }
        }
        DensityFunction.Ap2 = Ap2;
        class Spline extends DensityFunction {
            spline;
            constructor(spline) {
                super();
                this.spline = spline;
            }
            compute(context) {
                return this.spline.compute(context);
            }
            mapAll(visitor) {
                const newCubicSpline = this.spline.mapAll((fn) => {
                    if (fn instanceof DensityFunction) {
                        return fn.mapAll(visitor);
                    }
                    return fn;
                });
                newCubicSpline.calculateMinMax();
                return visitor.map(new Spline(newCubicSpline));
            }
            minValue() {
                return this.spline.min();
            }
            maxValue() {
                return this.spline.max();
            }
        }
        DensityFunction.Spline = Spline;
        class YClampedGradient extends DensityFunction {
            fromY;
            toY;
            fromValue;
            toValue;
            constructor(fromY, toY, fromValue, toValue) {
                super();
                this.fromY = fromY;
                this.toY = toY;
                this.fromValue = fromValue;
                this.toValue = toValue;
            }
            compute(context) {
                return clampedMap(context.y, this.fromY, this.toY, this.fromValue, this.toValue);
            }
            minValue() {
                return Math.min(this.fromValue, this.toValue);
            }
            maxValue() {
                return Math.max(this.fromValue, this.toValue);
            }
        }
        DensityFunction.YClampedGradient = YClampedGradient;
    })(DensityFunction || (DensityFunction = {}));

    class FluidStatus {
        level;
        type;
        constructor(level, type) {
            this.level = level;
            this.type = type;
        }
        at(level) {
            return level < this.level ? this.type : BlockState.AIR;
        }
    }
    var Aquifer;
    (function (Aquifer) {
        function createDisabled(fluidPicker) {
            return {
                compute({ x, y, z }, density) {
                    if (density > 0) {
                        return undefined;
                    }
                    return fluidPicker(x, y, z).at(y);
                },
            };
        }
        Aquifer.createDisabled = createDisabled;
    })(Aquifer || (Aquifer = {}));
    class NoiseAquifer {
        noiseChunk;
        router;
        random;
        globalFluidPicker;
        static X_SPACING = 16;
        static Y_SPACING = 12;
        static Z_SPACING = 16;
        static SURFACE_SAMPLING = [[-2, -1], [-1, -1], [0, -1], [1, -1], [-3, 0], [-2, 0], [-1, 0], [0, 0], [1, 0], [-2, 1], [-1, 1], [0, 1], [1, 1]];
        minGridX;
        minGridY;
        minGridZ;
        gridSizeX;
        gridSizeZ;
        gridSize;
        aquiferCache;
        aquiferLocationCache;
        constructor(noiseChunk, chunkPos, router, random, minY, height, globalFluidPicker) {
            this.noiseChunk = noiseChunk;
            this.router = router;
            this.random = random;
            this.globalFluidPicker = globalFluidPicker;
            this.minGridX = this.gridX(ChunkPos.minBlockX(chunkPos)) - 1;
            this.gridSizeX = this.gridX(ChunkPos.maxBlockX(chunkPos)) + 1 - this.minGridX + 1;
            this.minGridY = this.gridY(minY) - 1;
            this.minGridZ = this.gridZ(ChunkPos.minBlockZ(chunkPos)) - 1;
            this.gridSizeZ = this.gridZ(ChunkPos.maxBlockZ(chunkPos)) + 1 - this.minGridZ + 1;
            const gridSizeY = this.gridY(minY + height) + 1 - this.minGridY + 1;
            this.gridSize = this.gridSizeX * gridSizeY * this.gridSizeZ;
            this.aquiferCache = Array(this.gridSize).fill(undefined);
            this.aquiferLocationCache = Array(this.gridSize).fill(BlockPos.ZERO);
        }
        compute({ x, y, z }, density) {
            if (density <= 0) {
                if (this.globalFluidPicker(x, y, z).at(y).is(BlockState.LAVA)) {
                    return BlockState.LAVA;
                }
                else {
                    const gridX = this.gridX(x - 5);
                    const gridY = this.gridY(y + 1);
                    const gridZ = this.gridZ(z - 5);
                    let mag1 = Number.MAX_SAFE_INTEGER;
                    let mag2 = Number.MAX_SAFE_INTEGER;
                    let mag3 = Number.MAX_SAFE_INTEGER;
                    let loc1 = BlockPos.ZERO;
                    let loc2 = BlockPos.ZERO;
                    let loc3 = BlockPos.ZERO;
                    for (let xOffset = 0; xOffset <= 1; xOffset += 1) {
                        for (let yOffset = -1; yOffset <= 1; yOffset += 1) {
                            for (let zOffset = 0; zOffset <= 1; zOffset += 1) {
                                const location = this.getLocation(gridX + xOffset, gridY + yOffset, gridZ + zOffset);
                                const magnitude = BlockPos.magnitude(location);
                                if (mag1 >= magnitude) {
                                    loc3 = loc2;
                                    loc2 = loc1;
                                    loc1 = location;
                                    mag3 = mag2;
                                    mag2 = mag1;
                                    mag1 = magnitude;
                                }
                                else if (mag2 >= magnitude) {
                                    loc3 = loc2;
                                    loc2 = location;
                                    mag3 = mag2;
                                    mag2 = magnitude;
                                }
                                else if (mag3 >= magnitude) {
                                    loc3 = location;
                                    mag3 = magnitude;
                                }
                            }
                        }
                    }
                    const status1 = this.getStatus(loc1);
                    const status2 = this.getStatus(loc2);
                    const status3 = this.getStatus(loc3);
                    const similarity12 = NoiseAquifer.similarity(mag1, mag2);
                    const similarity13 = NoiseAquifer.similarity(mag1, mag3);
                    const similarity23 = NoiseAquifer.similarity(mag2, mag3);
                    let pressure;
                    if (status1.at(y).is(BlockState.WATER) && this.globalFluidPicker(x, y - 1, z).at(y - 1).is(BlockState.LAVA)) {
                        pressure = 1;
                    }
                    else if (similarity12 > -1) {
                        const barrier = lazy$1(() => this.router.barrier.compute(DensityFunction.context(x, y * 0.5, z)));
                        const pressure12 = this.calculatePressure(y, status1, status2, barrier);
                        const pressure13 = this.calculatePressure(y, status1, status3, barrier);
                        const pressure23 = this.calculatePressure(y, status2, status3, barrier);
                        const n = Math.max(pressure12, pressure13 * Math.max(0, similarity13), pressure23 * Math.max(similarity23));
                        pressure = Math.max(0, 2 * Math.max(0, similarity12) * n);
                    }
                    else {
                        pressure = 0;
                    }
                    if (density + pressure <= 0) {
                        return status1.at(y);
                    }
                }
            }
            return undefined;
        }
        static similarity(a, b) {
            return 1 - Math.abs(b - a) / 25;
        }
        calculatePressure(y, status1, status2, barrier) {
            const fluid1 = status1.at(y);
            const fluid2 = status2.at(y);
            if ((fluid1.is(BlockState.LAVA) && fluid2.is(BlockState.WATER)) || (fluid1.is(BlockState.WATER) && fluid2.is(BlockState.LAVA))) {
                return 1;
            }
            const levelDiff = Math.abs(status1.level - status2.level);
            if (levelDiff === 0) {
                return 0;
            }
            const levelAvg = (status1.level + status2.level) / 2;
            const levelAvgDiff = y + 0.5 - levelAvg;
            const p = levelDiff / 2 - Math.abs(levelAvgDiff);
            const pressure = levelAvgDiff > 0
                ? p > 0 ? p / 1.5 : p / 2.5
                : p > -3 ? (p + 3) / 3 : (p + 3) / 10;
            if (pressure < -2 || pressure > 2) {
                return pressure;
            }
            return pressure + barrier();
        }
        getStatus(location) {
            const [x, y, z] = location;
            const index = this.getIndex(this.gridX(x), this.gridY(y), this.gridZ(z));
            const cachedStatus = this.aquiferCache[index];
            if (cachedStatus !== undefined) {
                return cachedStatus;
            }
            const status = this.computeStatus(x, y, z);
            this.aquiferCache[index] = status;
            return status;
        }
        computeStatus(x, y, z) {
            const globalStatus = this.globalFluidPicker(x, y, z);
            let minPreliminarySurface = Number.MAX_SAFE_INTEGER;
            let isAquifer = false;
            for (const [xOffset, zOffset] of NoiseAquifer.SURFACE_SAMPLING) {
                const blockX = x + (zOffset << 4);
                const blockZ = z + (zOffset << 4);
                const preliminarySurface = this.noiseChunk.getPreliminarySurfaceLevel(blockX, blockZ);
                minPreliminarySurface = Math.min(minPreliminarySurface, preliminarySurface);
                const noOffset = xOffset === 0 && zOffset === 0;
                if (noOffset && y - 12 > preliminarySurface + 8) {
                    return globalStatus;
                }
                if ((noOffset || y + 12 > preliminarySurface + 8)) {
                    const newStatus = this.globalFluidPicker(blockX, preliminarySurface + 8, blockZ);
                    if (!newStatus.at(preliminarySurface + 8).is(BlockState.AIR)) {
                        if (noOffset) {
                            return newStatus;
                        }
                        else {
                            isAquifer = true;
                        }
                    }
                }
            }
            const allowedFloodedness = isAquifer ? clampedMap(minPreliminarySurface + 8 - y, 0, 64, 1, 0) : 0;
            const floodedness = clamp$1(this.router.fluidLevelFloodedness.compute(DensityFunction.context(x, y * 0.67, z)), -1, 1);
            if (floodedness > map(allowedFloodedness, 1, 0, -0.3, 0.8)) {
                return globalStatus;
            }
            if (floodedness <= map(allowedFloodedness, 1, 0, -0.8, 0.4)) {
                return new FluidStatus(Number.MIN_SAFE_INTEGER, globalStatus.type);
            }
            const gridY = Math.floor(y / 40);
            const spread = this.router.fluidLevelSpread.compute(DensityFunction.context(Math.floor(x / 16), gridY, Math.floor(z / 16)));
            const level = gridY * 40 + 20 + Math.floor(spread / 3) * 3;
            const statusLevel = Math.min(minPreliminarySurface, level);
            const fluid = this.getFluidType(x, y, z, globalStatus.type, level);
            return new FluidStatus(statusLevel, fluid);
        }
        getFluidType(x, y, z, global, level) {
            if (level <= -10) {
                const lava = this.router.lava.compute(DensityFunction.context(Math.floor(x / 64), Math.floor(y / 40), Math.floor(z / 64)));
                if (Math.abs(lava) > 0.3) {
                    return BlockState.LAVA;
                }
            }
            return global;
        }
        getLocation(x, y, z) {
            const index = this.getIndex(x, y, z);
            const cachedLocation = this.aquiferLocationCache[index];
            if (BlockPos.equals(cachedLocation, BlockPos.ZERO)) {
                return cachedLocation;
            }
            const random = this.random.at(x, y, z);
            const location = BlockPos.create(x * NoiseAquifer.X_SPACING + random.nextInt(10), y * NoiseAquifer.Y_SPACING + random.nextInt(9), z * NoiseAquifer.Z_SPACING + random.nextInt(10));
            this.aquiferLocationCache[index] = location;
            return location;
        }
        getIndex(x, y, z) {
            const gridX = x - this.minGridX;
            const gridY = y - this.minGridY;
            const gridZ = z - this.minGridZ;
            const index = (gridY * this.gridSizeZ + gridZ) * this.gridSizeX + gridX;
            if (index < 0 || index >= this.gridSize) {
                throw new Error(`Invalid aquifer index at ${x} ${y} ${z}: 0 <= ${index} < ${this.gridSize}`);
            }
            return index;
        }
        gridX(x) {
            return Math.floor(x / NoiseAquifer.X_SPACING);
        }
        gridY(y) {
            return Math.floor(y / NoiseAquifer.Y_SPACING);
        }
        gridZ(z) {
            return Math.floor(z / NoiseAquifer.Z_SPACING);
        }
    }

    var HeightProvider;
    (function (HeightProvider) {
        function fromJson(obj) {
            const root = Json.readObject(obj) ?? {};
            const type = Json.readString(root.type)?.replace(/^minecraft:/, '');
            switch (type) {
                case undefined: return constant(VerticalAnchor.fromJson(obj));
                case 'constant': return constant(VerticalAnchor.fromJson(root.value));
                case 'uniform': return uniform(VerticalAnchor.fromJson(root.min_inclusive), VerticalAnchor.fromJson(root.max_inclusive));
                case 'biased_to_bottom': return biased_to_bottom(VerticalAnchor.fromJson(root.min_inclusive), VerticalAnchor.fromJson(root.max_inclusive), Json.readInt(root.inner));
                case 'very_biased_to_bottom': return very_biased_to_bottom(VerticalAnchor.fromJson(root.min_inclusive), VerticalAnchor.fromJson(root.max_inclusive), Json.readInt(root.inner));
                case 'trapezoid': return trapezoid(VerticalAnchor.fromJson(root.min_inclusive), VerticalAnchor.fromJson(root.max_inclusive), Json.readInt(root.plateau));
                case 'weighted_list':
                    return weighted_list(Json.readArray(root.distribution, (obj) => {
                        const entry = Json.readObject(obj) ?? {};
                        return { weight: Json.readInt(entry.weight) ?? 1, data: fromJson(entry.data) };
                    }) ?? []);
            }
            return () => 0;
        }
        HeightProvider.fromJson = fromJson;
        function constant(anchor) {
            return (_, context) => anchor(context);
        }
        HeightProvider.constant = constant;
        function uniform(minInclusive, maxInclusive) {
            return (random, context) => {
                const minY = minInclusive(context);
                const maxY = maxInclusive(context);
                if (minY > maxY) {
                    return minY;
                }
                else {
                    return random.nextInt(maxY - minY + 1) + minY;
                }
            };
        }
        HeightProvider.uniform = uniform;
        function biased_to_bottom(minInclusive, maxInclusive, inner = 1) {
            return (random, context) => {
                const minY = minInclusive(context);
                const maxY = maxInclusive(context);
                if (maxY - minY - inner + 1 <= 0) {
                    return minY;
                }
                else {
                    const r = random.nextInt(maxY - minY - inner + 1);
                    return random.nextInt(r + inner) + minY;
                }
            };
        }
        HeightProvider.biased_to_bottom = biased_to_bottom;
        function very_biased_to_bottom(minInclusive, maxInclusive, inner = 1) {
            return (random, context) => {
                const minY = minInclusive(context);
                const maxY = maxInclusive(context);
                if (maxY - minY - inner + 1 <= 0) {
                    return minY;
                }
                else {
                    const r1 = nextInt(random, minY + inner, maxY);
                    const r2 = nextInt(random, minY, r1 - 1);
                    return nextInt(random, minY, r2 - 1 + inner);
                }
            };
        }
        HeightProvider.very_biased_to_bottom = very_biased_to_bottom;
        function trapezoid(minInclusive, maxInclusive, plateau = 0) {
            return (random, context) => {
                const minY = minInclusive(context);
                const maxY = maxInclusive(context);
                if (minY > maxY) {
                    return minY;
                }
                else {
                    const range = maxY - minY;
                    if (plateau >= range) {
                        return randomBetweenInclusive(random, minY, maxY);
                    }
                    else {
                        const slope = (range - plateau) / 2;
                        const r = range - slope;
                        return minY + randomBetweenInclusive(random, 0, r) + randomBetweenInclusive(random, 0, slope);
                    }
                }
            };
        }
        HeightProvider.trapezoid = trapezoid;
        function weighted_list(distribution) {
            const totalWeight = distribution.reduce((sum, e, i) => sum + e.weight, 0);
            return (random, context) => {
                let r = random.nextInt(totalWeight);
                for (const e of distribution) {
                    r -= e.weight;
                    if (r <= 0) {
                        return e.data(random, context);
                    }
                }
                return 0;
            };
        }
        HeightProvider.weighted_list = weighted_list;
    })(HeightProvider || (HeightProvider = {}));

    var Heightmap;
    (function (Heightmap) {
        function fromJson(obj) {
            if (typeof obj === 'string') {
                if (obj === 'WORLD_SURFACE_WG' || obj === 'WORLD_SURFACE' || obj === 'OCEAN_FLOOR_WG' || obj === 'OCEAN_FLOOR' || obj === 'MOTION_BLOCKING' || obj === 'MOTION_BLOCKING_NO_LEAVES') {
                    return obj;
                }
            }
        }
        Heightmap.fromJson = fromJson;
    })(Heightmap || (Heightmap = {}));

    class NoiseChunk {
        cellCountXZ;
        cellCountY;
        cellNoiseMinY;
        minX;
        minZ;
        settings;
        cellWidth;
        cellHeight;
        firstCellX;
        firstCellZ;
        firstNoiseX;
        firstNoiseZ;
        noiseSizeXZ;
        preliminarySurfaceLevelCache = new Map();
        aquifer;
        materialRule;
        preliminarySurfaceLevel;
        constructor(cellCountXZ, cellCountY, cellNoiseMinY, randomState, minX, minZ, settings, aquifersEnabled, fluidPicker) {
            this.cellCountXZ = cellCountXZ;
            this.cellCountY = cellCountY;
            this.cellNoiseMinY = cellNoiseMinY;
            this.minX = minX;
            this.minZ = minZ;
            this.settings = settings;
            this.cellWidth = NoiseSettings.cellWidth(settings);
            this.cellHeight = NoiseSettings.cellHeight(settings);
            this.firstCellX = Math.floor(minX / this.cellWidth);
            this.firstCellZ = Math.floor(minZ / this.cellWidth);
            this.firstNoiseX = minX >> 2;
            this.firstNoiseZ = minZ >> 2;
            this.noiseSizeXZ = (cellCountXZ * this.cellWidth) >> 2;
            if (!aquifersEnabled || true) { // WIP: Noise aquifers don't work yet
                this.aquifer = Aquifer.createDisabled(fluidPicker);
            }
            else {
                const chunkPos = ChunkPos.fromBlockPos(BlockPos.create(minX, 0, minZ));
                const minY = cellNoiseMinY * NoiseSettings.cellHeight(settings);
                const height = cellCountY * NoiseSettings.cellHeight(settings);
                this.aquifer = new NoiseAquifer(this, chunkPos, randomState.router, randomState.aquiferRandom, minY, height, fluidPicker);
            }
            const finalDensity = randomState.router.finalDensity;
            this.materialRule = MaterialRule.fromList([
                (context) => this.aquifer.compute(context, finalDensity.compute(context)),
            ]);
            this.preliminarySurfaceLevel = randomState.router.preliminarySurfaceLevel;
        }
        getFinalState(x, y, z) {
            return this.materialRule({ x, y, z });
        }
        getPreliminarySurfaceLevel(quartX, quartZ) {
            return computeIfAbsent(this.preliminarySurfaceLevelCache, ChunkPos.asLong(quartX, quartZ), () => {
                const x = quartX << 2;
                const z = quartZ << 2;
                return Math.floor(this.preliminarySurfaceLevel.compute(DensityFunction.context(x, 0, z)));
            });
        }
    }
    var MaterialRule;
    (function (MaterialRule) {
        function fromList(rules) {
            return (context) => {
                for (const rule of rules) {
                    const state = rule(context);
                    if (state)
                        return state;
                }
                return undefined;
            };
        }
        MaterialRule.fromList = fromList;
    })(MaterialRule || (MaterialRule = {}));

    class NoiseChunkGenerator {
        biomeSource;
        settings;
        noiseChunkCache;
        globalFluidPicker;
        constructor(biomeSource, settings) {
            this.biomeSource = biomeSource;
            this.settings = settings;
            this.noiseChunkCache = new Map();
            const lavaFluid = new FluidStatus(-54, BlockState.LAVA);
            const defaultFluid = new FluidStatus(settings.seaLevel, settings.defaultFluid);
            this.globalFluidPicker = (x, y, z) => {
                if (y < Math.min(-54, settings.seaLevel)) {
                    return lavaFluid;
                }
                return defaultFluid;
            };
        }
        getBaseHeight(blockX, blockZ, heightmap, randomState) {
            let predicate;
            if (heightmap === 'OCEAN_FLOOR' || heightmap === 'OCEAN_FLOOR_WG') {
                predicate = (state) => state.equals(BlockState.STONE);
            }
            else {
                predicate = (state) => !state.equals(BlockState.AIR);
            }
            return this.iterateNoiseColumn(randomState, blockX, blockZ, undefined, predicate, BlockState.STONE) ?? this.settings.noise.minY;
        }
        iterateNoiseColumn(randomState, blockX, blockZ, fillArray, predicate, defaultBlock) {
            const minY = this.settings.noise.minY;
            const cellHeight = NoiseSettings.cellHeight(this.settings.noise);
            const minCellY = Math.floor(minY / cellHeight);
            const cellCountY = Math.floor(this.settings.noise.height / cellHeight);
            if (cellCountY <= 0) {
                return undefined;
            }
            const cellWidth = NoiseSettings.cellWidth(this.settings.noise);
            const cellX = Math.floor(blockX / cellWidth);
            const cellZ = Math.floor(blockZ / cellWidth);
            const noiseChunk = new NoiseChunk(1, cellCountY, minCellY, randomState, cellX, cellZ, this.settings.noise, this.settings.aquifersEnabled, this.globalFluidPicker);
            for (let cellY = cellCountY - 1; cellY >= 0; cellY -= 1) {
                for (let offY = cellHeight - 1; offY >= 0; offY -= 1) {
                    const blockY = (minCellY + cellY) * cellHeight + offY;
                    const state = noiseChunk.getFinalState(blockX, blockY, blockZ) ?? defaultBlock ?? this.settings.defaultBlock;
                    if (fillArray !== undefined) {
                        fillArray[blockY + minY] = state;
                    }
                    if (predicate !== undefined && predicate(state)) {
                        return blockY + 1;
                    }
                }
            }
        }
        fill(randomState, chunk, onlyFirstZ = false) {
            const minY = Math.max(chunk.minY, this.settings.noise.minY);
            const maxY = Math.min(chunk.maxY, this.settings.noise.minY + this.settings.noise.height);
            const cellWidth = NoiseSettings.cellWidth(this.settings.noise);
            const cellHeight = NoiseSettings.cellHeight(this.settings.noise);
            const cellCountXZ = Math.floor(16 / cellWidth);
            const minCellY = Math.floor(minY / cellHeight);
            const cellCountY = Math.floor((maxY - minY) / cellHeight);
            const minX = ChunkPos.minBlockX(chunk.pos);
            const minZ = ChunkPos.minBlockZ(chunk.pos);
            const noiseChunk = this.getOrCreateNoiseChunk(randomState, chunk);
            for (let cellX = 0; cellX < cellCountXZ; cellX += 1) {
                for (let cellZ = 0; cellZ < (onlyFirstZ ? 1 : cellCountXZ); cellZ += 1) {
                    let section = chunk.getOrCreateSection(chunk.sectionsCount - 1);
                    for (let cellY = cellCountY - 1; cellY >= 0; cellY -= 1) {
                        for (let offY = cellHeight - 1; offY >= 0; offY -= 1) {
                            const blockY = (minCellY + cellY) * cellHeight + offY;
                            const sectionY = blockY & 0xF;
                            const sectionIndex = chunk.getSectionIndex(blockY);
                            if (chunk.getSectionIndex(section.minBlockY) !== sectionIndex) {
                                section = chunk.getOrCreateSection(sectionIndex);
                            }
                            for (let offX = 0; offX < cellWidth; offX += 1) {
                                const blockX = minX + cellX * cellWidth + offX;
                                const sectionX = blockX & 0xF;
                                for (let offZ = 0; offZ < (onlyFirstZ ? 1 : cellWidth); offZ += 1) {
                                    const blockZ = minZ + cellZ * cellWidth + offZ;
                                    const sectionZ = blockZ & 0xF;
                                    const state = noiseChunk.getFinalState(blockX, blockY, blockZ) ?? this.settings.defaultBlock;
                                    section.setBlockState(sectionX, sectionY, sectionZ, state);
                                }
                            }
                        }
                    }
                }
            }
        }
        buildSurface(randomState, chunk, /** @deprecated */ biome = 'minecraft:plains') {
            const noiseChunk = this.getOrCreateNoiseChunk(randomState, chunk);
            const context = this.settings.noise;
            randomState.surfaceSystem.buildSurface(chunk, noiseChunk, context, () => biome);
        }
        computeBiome(randomState, quartX, quartY, quartZ) {
            return this.biomeSource.getBiome(quartX, quartY, quartZ, randomState.sampler);
        }
        getOrCreateNoiseChunk(randomState, chunk) {
            return computeIfAbsent(this.noiseChunkCache, ChunkPos.toLong(chunk.pos), () => {
                const minY = Math.max(chunk.minY, this.settings.noise.minY);
                const maxY = Math.min(chunk.maxY, this.settings.noise.minY + this.settings.noise.height);
                const cellWidth = NoiseSettings.cellWidth(this.settings.noise);
                const cellHeight = NoiseSettings.cellHeight(this.settings.noise);
                const cellCountXZ = Math.floor(16 / cellWidth);
                const minCellY = Math.floor(minY / cellHeight);
                const cellCountY = Math.floor((maxY - minY) / cellHeight);
                const minX = ChunkPos.minBlockX(chunk.pos);
                const minZ = ChunkPos.minBlockZ(chunk.pos);
                return new NoiseChunk(cellCountXZ, cellCountY, minCellY, randomState, minX, minZ, this.settings.noise, this.settings.aquifersEnabled, this.globalFluidPicker);
            });
        }
    }

    class CheckerboardBiomeSource {
        shift;
        biomes;
        n;
        constructor(shift, biomes) {
            this.shift = shift;
            this.biomes = biomes;
            if (biomes.length === 0) {
                throw new Error('Cannot create checkerboard biome source without biomes');
            }
            this.n = biomes.length;
        }
        getBiome(x, y, z) {
            const i = (((x >> this.shift) + (z >> this.shift)) % this.n + this.n) % this.n;
            return Identifier.parse(this.biomes[i].toString());
        }
        static fromJson(obj) {
            const root = Json.readObject(obj) ?? {};
            const scale = Json.readInt(root.scale) ?? 2;
            let biomes;
            if (typeof root.biomes === 'string') {
                biomes = [Identifier.parse(root.biomes)];
            }
            else {
                biomes = Json.readArray(root.biomes, (b) => Identifier.parse(Json.readString(b) ?? '')) ?? [];
            }
            return new CheckerboardBiomeSource(scale + 2, biomes);
        }
    }

    class FixedBiomeSource {
        biome;
        constructor(biome) {
            this.biome = biome;
        }
        getBiome() {
            return this.biome;
        }
        static fromJson(obj) {
            const root = Json.readObject(obj) ?? {};
            const biome = Identifier.parse(Json.readString(root.biome) ?? 'plains');
            return new FixedBiomeSource(biome);
        }
    }

    var Climate;
    (function (Climate) {
        const PARAMETER_SPACE = 7;
        function target(temperature, humidity, continentalness, erosion, depth, weirdness) {
            return new TargetPoint(temperature, humidity, continentalness, erosion, depth, weirdness);
        }
        Climate.target = target;
        function parameters(temperature, humidity, continentalness, erosion, depth, weirdness, offset) {
            return new ParamPoint(param(temperature), param(humidity), param(continentalness), param(erosion), param(depth), param(weirdness), offset);
        }
        Climate.parameters = parameters;
        function param(value, max) {
            if (typeof value === 'number') {
                return new Param(value, max ?? value);
            }
            return value;
        }
        Climate.param = param;
        class Param {
            min;
            max;
            constructor(min, max) {
                this.min = min;
                this.max = max;
            }
            distance(param) {
                const diffMax = (typeof param === 'number' ? param : param.min) - this.max;
                const diffMin = this.min - (typeof param === 'number' ? param : param.max);
                if (diffMax > 0) {
                    return diffMax;
                }
                return Math.max(diffMin, 0);
            }
            union(param) {
                return new Param(Math.min(this.min, param.min), Math.max(this.max, param.max));
            }
            static fromJson(obj) {
                if (typeof obj === 'number')
                    return new Param(obj, obj);
                const [min, max] = Json.readArray(obj, e => Json.readNumber(e)) ?? [];
                return new Param(min ?? 0, max ?? 0);
            }
        }
        Climate.Param = Param;
        class ParamPoint {
            temperature;
            humidity;
            continentalness;
            erosion;
            depth;
            weirdness;
            offset;
            constructor(temperature, humidity, continentalness, erosion, depth, weirdness, offset) {
                this.temperature = temperature;
                this.humidity = humidity;
                this.continentalness = continentalness;
                this.erosion = erosion;
                this.depth = depth;
                this.weirdness = weirdness;
                this.offset = offset;
            }
            fittness(point) {
                return square(this.temperature.distance(point.temperature))
                    + square(this.humidity.distance(point.humidity))
                    + square(this.continentalness.distance(point.continentalness))
                    + square(this.erosion.distance(point.erosion))
                    + square(this.depth.distance(point.depth))
                    + square(this.weirdness.distance(point.weirdness))
                    + square(this.offset - point.offset);
            }
            space() {
                return [this.temperature, this.humidity, this.continentalness, this.erosion, this.depth, this.weirdness, new Param(this.offset, this.offset)];
            }
            static fromJson(obj) {
                const root = Json.readObject(obj) ?? {};
                return new ParamPoint(Param.fromJson(root.temperature), Param.fromJson(root.humidity), Param.fromJson(root.continentalness), Param.fromJson(root.erosion), Param.fromJson(root.depth), Param.fromJson(root.weirdness), Json.readNumber(root.offset) ?? 0);
            }
        }
        Climate.ParamPoint = ParamPoint;
        class TargetPoint {
            temperature;
            humidity;
            continentalness;
            erosion;
            depth;
            weirdness;
            constructor(temperature, humidity, continentalness, erosion, depth, weirdness) {
                this.temperature = temperature;
                this.humidity = humidity;
                this.continentalness = continentalness;
                this.erosion = erosion;
                this.depth = depth;
                this.weirdness = weirdness;
            }
            get offset() {
                return 0;
            }
            toArray() {
                return [this.temperature, this.humidity, this.continentalness, this.erosion, this.depth, this.weirdness, this.offset];
            }
        }
        Climate.TargetPoint = TargetPoint;
        class Parameters {
            things;
            index;
            constructor(things) {
                this.things = things;
                this.index = new RTree(things);
            }
            find(target) {
                return this.index.search(target, (node, values) => node.distance(values));
            }
        }
        Climate.Parameters = Parameters;
        class Sampler {
            temperature;
            humidity;
            continentalness;
            erosion;
            depth;
            weirdness;
            constructor(temperature, humidity, continentalness, erosion, depth, weirdness) {
                this.temperature = temperature;
                this.humidity = humidity;
                this.continentalness = continentalness;
                this.erosion = erosion;
                this.depth = depth;
                this.weirdness = weirdness;
            }
            static fromRouter(router) {
                return new Climate.Sampler(router.temperature, router.vegetation, router.continents, router.erosion, router.depth, router.ridges);
            }
            sample(x, y, z) {
                const context = DensityFunction.context(x << 2, y << 2, z << 2);
                return Climate.target(this.temperature.compute(context), this.humidity.compute(context), this.continentalness.compute(context), this.erosion.compute(context), this.depth.compute(context), this.weirdness.compute(context));
            }
        }
        Climate.Sampler = Sampler;
        class RTree {
            static CHILDREN_PER_NODE = 10;
            root;
            last_leaf = null;
            constructor(points) {
                if (points.length === 0) {
                    throw new Error('At least one point is required to build search tree');
                }
                this.root = RTree.build(points.map(([point, thing]) => new RLeaf(point, thing)));
            }
            static build(nodes) {
                if (nodes.length === 1) {
                    return nodes[0];
                }
                if (nodes.length <= RTree.CHILDREN_PER_NODE) {
                    const sortedNodes = nodes
                        .map(node => {
                        let key = 0.0;
                        for (let i = 0; i < PARAMETER_SPACE; i += 1) {
                            const param = node.space[i];
                            key += Math.abs((param.min + param.max) / 2.0);
                        }
                        return { key, node };
                    })
                        .sort((a, b) => a.key - b.key)
                        .map(({ node }) => node);
                    return new RSubTree(sortedNodes);
                }
                let f = Infinity;
                let n3 = -1;
                let result = [];
                for (let n2 = 0; n2 < PARAMETER_SPACE; ++n2) {
                    nodes = RTree.sort(nodes, n2, false);
                    result = RTree.bucketize(nodes);
                    let f2 = 0.0;
                    for (const subTree2 of result) {
                        f2 += RTree.area(subTree2.space);
                    }
                    if (!(f > f2))
                        continue;
                    f = f2;
                    n3 = n2;
                }
                nodes = RTree.sort(nodes, n3, false);
                result = RTree.bucketize(nodes);
                result = RTree.sort(result, n3, true);
                return new RSubTree(result.map(subTree => RTree.build(subTree.children)));
            }
            static sort(nodes, i, abs) {
                return nodes
                    .map(node => {
                    const param = node.space[i];
                    const f = (param.min + param.max) / 2;
                    const key = abs ? Math.abs(f) : f;
                    return { key, node };
                })
                    .sort((a, b) => a.key - b.key)
                    .map(({ node }) => node);
            }
            static bucketize(nodes) {
                const arrayList = [];
                let arrayList2 = [];
                const n = Math.pow(10.0, Math.floor(Math.log(nodes.length - 0.01) / Math.log(10.0)));
                for (const node of nodes) {
                    arrayList2.push(node);
                    if (arrayList2.length < n)
                        continue;
                    arrayList.push(new RSubTree(arrayList2));
                    arrayList2 = [];
                }
                if (arrayList2.length !== 0) {
                    arrayList.push(new RSubTree(arrayList2));
                }
                return arrayList;
            }
            static area(params) {
                let f = 0.0;
                for (const param of params) {
                    f += Math.abs(param.max - param.min);
                }
                return f;
            }
            search(target, distance) {
                const leaf = this.root.search(target.toArray(), this.last_leaf, distance);
                this.last_leaf = leaf;
                return leaf.thing();
            }
        }
        Climate.RTree = RTree;
        class RNode {
            space;
            constructor(space) {
                this.space = space;
            }
            distance(values) {
                let result = 0;
                for (let i = 0; i < PARAMETER_SPACE; i += 1) {
                    result += square(this.space[i].distance(values[i]));
                }
                return result;
            }
        }
        Climate.RNode = RNode;
        class RSubTree extends RNode {
            children;
            constructor(children) {
                super(RSubTree.buildSpace(children));
                this.children = children;
            }
            static buildSpace(nodes) {
                let space = [...Array(PARAMETER_SPACE)].map(() => new Param(Infinity, -Infinity));
                for (const node of nodes) {
                    space = [...Array(PARAMETER_SPACE)].map((_, i) => space[i].union(node.space[i]));
                }
                return space;
            }
            search(values, closest_leaf, distance) {
                let dist = closest_leaf ? distance(closest_leaf, values) : Infinity;
                let leaf = closest_leaf;
                for (const node of this.children) {
                    const d1 = distance(node, values);
                    if (dist <= d1)
                        continue;
                    const leaf2 = node.search(values, leaf, distance);
                    if (leaf2 === null)
                        continue;
                    const d2 = node == leaf2 ? d1 : distance(leaf2, values);
                    if (d2 === 0)
                        return leaf2;
                    if (dist <= d2)
                        continue;
                    dist = d2;
                    leaf = leaf2;
                }
                return leaf;
            }
        }
        Climate.RSubTree = RSubTree;
        class RLeaf extends RNode {
            thing;
            constructor(point, thing) {
                super(point.space());
                this.thing = thing;
            }
            search() {
                return this;
            }
        }
        Climate.RLeaf = RLeaf;
    })(Climate || (Climate = {}));

    class MultiNoiseBiomeSource {
        parameters;
        constructor(entries) {
            this.parameters = new Climate.Parameters(entries);
        }
        getBiome(x, y, z, climateSampler) {
            const target = climateSampler.sample(x, y, z);
            return this.parameters.find(target);
        }
        static fromJson(obj) {
            const root = Json.readObject(obj) ?? {};
            const biomes = Json.readArray(root.biomes, b => (b => ({
                biome: Identifier.parse(Json.readString(b.biome) ?? 'plains'),
                parameters: Climate.ParamPoint.fromJson(b.parameters),
            }))(Json.readObject(b) ?? {})) ?? [];
            const entries = biomes.map(b => [b.parameters, () => b.biome]);
            return new MultiNoiseBiomeSource(entries);
        }
    }

    class TheEndBiomeSource {
        static END = Identifier.create('the_end');
        static HIGHLANDS = Identifier.create('end_highlands');
        static MIDLANDS = Identifier.create('end_midlands');
        static ISLANDS = Identifier.create('small_end_islands');
        static BARRENS = Identifier.create('end_barrens');
        getBiome(x, y, z, climateSampler) {
            const blockX = x << 2;
            const blockY = y << 2;
            const blockZ = z << 2;
            const sectionX = blockX >> 4;
            const sectionZ = blockZ >> 4;
            if (sectionX * sectionX + sectionZ * sectionZ <= 4096) {
                return TheEndBiomeSource.END;
            }
            const context = DensityFunction.context((sectionX * 2 + 1) * 8, blockY, (sectionZ * 2 + 1) * 8);
            const erosion = climateSampler.erosion.compute(context);
            if (erosion > 0.25) {
                return TheEndBiomeSource.HIGHLANDS;
            }
            else if (erosion >= -0.0625) {
                return TheEndBiomeSource.MIDLANDS;
            }
            else if (erosion >= -0.21875) {
                return TheEndBiomeSource.BARRENS;
            }
            else {
                return TheEndBiomeSource.ISLANDS;
            }
        }
        static fromJson(obj) {
            return new TheEndBiomeSource();
        }
    }

    var BiomeSource;
    (function (BiomeSource) {
        function fromJson(obj) {
            const root = Json.readObject(obj) ?? {};
            const type = Json.readString(root.type)?.replace(/^minecraft:/, '');
            switch (type) {
                case 'fixed': return FixedBiomeSource.fromJson(obj);
                case 'checkerboard': return CheckerboardBiomeSource.fromJson(obj);
                case 'multi_noise': return MultiNoiseBiomeSource.fromJson(obj);
                case 'the_end': return TheEndBiomeSource.fromJson(obj);
                default: return new FixedBiomeSource(Identifier.create('plains'));
            }
        }
        BiomeSource.fromJson = fromJson;
        function findBiomeHorizontal(biomeSource, centerX, y, centerZ, range, predicate, random, sampler, step = 1, searchFromCenter = false) {
            if (biomeSource instanceof FixedBiomeSource) {
                if (predicate(biomeSource.getBiome())) {
                    if (searchFromCenter) {
                        return { pos: BlockPos.create(centerX, y, centerZ), biome: biomeSource.getBiome() };
                    }
                    else {
                        return { pos: BlockPos.create(centerX - range + random.nextInt(range * 2 + 1), y, centerZ - range + random.nextInt(range * 2 + 1)), biome: biomeSource.getBiome() };
                    }
                }
                else {
                    return undefined;
                }
            }
            const centerQuardX = centerX >> 2;
            const centerQuardZ = centerZ >> 2;
            const quardRange = range >> 2;
            const quardY = y >> 2;
            var result = undefined;
            var found_count = 0;
            var currentRangeStart = searchFromCenter ? 0 : quardRange;
            for (var currentRange = currentRangeStart; currentRange <= quardRange; currentRange += step) {
                for (var quardZOffset = -currentRange; quardZOffset <= currentRange; quardZOffset += step) {
                    const isZEdge = Math.abs(quardZOffset) === currentRange;
                    for (var quardXOffset = -currentRange; quardXOffset <= currentRange; quardXOffset += step) {
                        if (searchFromCenter) {
                            const isXEdge = Math.abs(quardXOffset) === currentRange;
                            if (!isXEdge && !isZEdge) {
                                continue;
                            }
                        }
                        const quardX = centerQuardX + quardXOffset;
                        const quardZ = centerQuardZ + quardZOffset;
                        const biome = biomeSource.getBiome(quardX, quardY, quardZ, sampler);
                        if (predicate(biome)) {
                            if (result === undefined || random.nextInt(found_count + 1) <= 0.5) {
                                result = { pos: BlockPos.create(quardX << 2, y, quardZ << 2), biome };
                                if (searchFromCenter) {
                                    return result;
                                }
                            }
                            found_count++;
                        }
                    }
                }
            }
            return result;
        }
        BiomeSource.findBiomeHorizontal = findBiomeHorizontal;
    })(BiomeSource || (BiomeSource = {}));

    class RandomState {
        seed;
        noiseCache;
        randomCache;
        random;
        aquiferRandom;
        oreRandom;
        surfaceSystem;
        router;
        sampler;
        constructor(settings, seed) {
            this.seed = seed;
            this.noiseCache = new Map();
            this.randomCache = new Map();
            this.random = (settings.legacyRandomSource ? new LegacyRandom(seed) : XoroshiroRandom.create(seed)).forkPositional();
            this.aquiferRandom = this.random.fromHashOf(Identifier.create('aquifer').toString()).forkPositional();
            this.oreRandom = this.random.fromHashOf(Identifier.create('ore').toString()).forkPositional();
            this.surfaceSystem = new SurfaceSystem(settings.surfaceRule, settings.defaultBlock, seed);
            this.router = NoiseRouter.mapAll(settings.noiseRouter, this.createVisitor(settings.noise, settings.legacyRandomSource));
            this.sampler = Climate.Sampler.fromRouter(this.router);
        }
        createVisitor(noiseSettings, legacyRandom) {
            const mapped = new Map();
            const getNoise = (noise) => {
                const key = noise.key();
                if (key === undefined) {
                    throw new Error('Cannot create noise without key');
                }
                if (legacyRandom) {
                    if (key.equals(Identifier.create('temperature'))) {
                        return new NormalNoise(new LegacyRandom(this.seed + BigInt(0)), NoiseParameters.create(-7, [1, 1]));
                    }
                    if (key.equals(Identifier.create('vegetation'))) {
                        return new NormalNoise(new LegacyRandom(this.seed + BigInt(1)), NoiseParameters.create(-7, [1, 1]));
                    }
                    if (key.equals(Identifier.create('offset'))) {
                        return new NormalNoise(this.random.fromHashOf('offset'), NoiseParameters.create(0, [0]));
                    }
                }
                return this.getOrCreateNoise(key);
            };
            const visitor = {
                map: (fn) => {
                    if (fn instanceof DensityFunction.HolderHolder) {
                        const key = fn.holder.key();
                        if (key !== undefined && mapped.has(key.toString())) {
                            return mapped.get(key.toString());
                        }
                        else {
                            const value = fn.holder.value().mapAll(visitor);
                            if (key !== undefined) {
                                mapped.set(key.toString(), value);
                            }
                            return value;
                        }
                    }
                    if (fn instanceof DensityFunction.Interpolated) {
                        return fn.withCellSize(NoiseSettings.cellWidth(noiseSettings), NoiseSettings.cellHeight(noiseSettings));
                    }
                    if (fn instanceof DensityFunction.ShiftedNoise) {
                        return new DensityFunction.ShiftedNoise(fn.shiftX, fn.shiftY, fn.shiftZ, fn.xzScale, fn.yScale, fn.noiseData, getNoise(fn.noiseData));
                    }
                    if (fn instanceof DensityFunction.Noise) {
                        return new DensityFunction.Noise(fn.xzScale, fn.yScale, fn.noiseData, getNoise(fn.noiseData));
                    }
                    if (fn instanceof DensityFunction.ShiftNoise) {
                        return fn.withNewNoise(getNoise(fn.noiseData));
                    }
                    if (fn instanceof DensityFunction.WeirdScaledSampler) {
                        return new DensityFunction.WeirdScaledSampler(fn.input, fn.rarityValueMapper, fn.noiseData, getNoise(fn.noiseData));
                    }
                    if (fn instanceof DensityFunction.OldBlendedNoise) {
                        const oldBlendedNoiseRandom = legacyRandom ? new LegacyRandom(this.seed + BigInt(0)) : this.random.fromHashOf(Identifier.create('terrain').toString());
                        return new DensityFunction.OldBlendedNoise(fn.xzScale, fn.yScale, fn.xzFactor, fn.yFactor, fn.smearScaleMultiplier, new BlendedNoise(oldBlendedNoiseRandom, fn.xzScale, fn.yScale, fn.xzFactor, fn.yFactor, fn.smearScaleMultiplier));
                    }
                    if (fn instanceof DensityFunction.EndIslands) {
                        return new DensityFunction.EndIslands(this.seed);
                    }
                    if (fn instanceof DensityFunction.Mapped) {
                        return fn.withMinMax();
                    }
                    if (fn instanceof DensityFunction.Ap2) {
                        return fn.withMinMax();
                    }
                    return fn;
                },
            };
            return visitor;
        }
        getOrCreateNoise(id) {
            const noises = Registry.REGISTRY.getOrThrow(Identifier.create('worldgen/noise'));
            return computeIfAbsent(this.noiseCache, id.toString(), key => new NormalNoise(this.random.fromHashOf(key), noises.getOrThrow(id)));
        }
        getOrCreateRandom(id) {
            return computeIfAbsent(this.randomCache, id.toString(), key => this.random.fromHashOf(key).forkPositional());
        }
    }

    class StructurePoolElement {
        static fromJson(obj) {
            const root = Json.readObject(obj) ?? {};
            switch (Json.readString(root.element_type)?.replace(/^minecraft:/, '')) {
                case 'single_pool_element':
                case 'legacy_single_pool_element':
                    const id = Identifier.parse(Json.readString(root.location) ?? '');
                    const template = {
                        key: () => id,
                        value: () => Structure.REGISTRY.get(id) ?? Structure.EMPTY,
                    };
                    return new StructurePoolElement.SinlgePoolElement(template);
                case 'list_pool_element':
                    const elements = Json.readArray('elements', StructurePoolElement.fromJson) ?? [];
                    return new StructurePoolElement.ListPoolElement(elements);
                case 'feature_pool_element':
                    return new StructurePoolElement.FeaturePoolElement();
                case 'empty_pool_element':
                default:
                    return new StructurePoolElement.EmptyPoolElement();
            }
        }
    }
    (function (StructurePoolElement) {
        class EmptyPoolElement extends StructurePoolElement {
            getBoundingBox(pos, rotation) {
                throw new Error('Invalid call of EmptyPoolElement');
            }
            getShuffledJigsawBlocks(rotation, random) {
                return [];
            }
            toString() {
                return '[Empty Pool Element]';
            }
        }
        StructurePoolElement.EmptyPoolElement = EmptyPoolElement;
        class FeaturePoolElement extends StructurePoolElement {
            defaultJigsawNBT;
            constructor() {
                super();
                const compoundMap = new Map();
                compoundMap.set('name', new NbtString('minecraft:bottom'));
                compoundMap.set('final_state', new NbtString('minecraft:air'));
                compoundMap.set('pool', new NbtString('minecraft:empty'));
                compoundMap.set('target', new NbtString('minecraft:empty'));
                compoundMap.set('joint', new NbtString('rollable'));
                this.defaultJigsawNBT = new NbtCompound(compoundMap);
            }
            getBoundingBox(pos, rotation) {
                return [pos, pos];
            }
            getShuffledJigsawBlocks(rotation, random) {
                return [{
                        pos: [0, 0, 0],
                        state: new BlockState(Identifier.create('jigsaw'), {
                            orientation: 'down_south',
                        }),
                        nbt: this.defaultJigsawNBT,
                    }];
            }
            toString() {
                return '[Feature Pool Element]';
            }
        }
        StructurePoolElement.FeaturePoolElement = FeaturePoolElement;
        class SinlgePoolElement extends StructurePoolElement {
            template;
            static JIGSAW_ID = Identifier.parse('jigsaw');
            constructor(template) {
                super();
                this.template = template;
            }
            getBoundingBox(pos, rotation) {
                const size = BlockPos.offset(this.template.value().getSize(), -1, -1, -1);
                const pos1 = pos;
                const pos2 = BlockPos.add(Structure.transform(size, rotation, BlockPos.ZERO), pos);
                const minPos = BlockPos.create(Math.min(pos1[0], pos2[0]), pos1[1], Math.min(pos1[2], pos2[2]));
                const maxPos = BlockPos.create(Math.max(pos1[0], pos2[0]), pos2[1], Math.max(pos1[2], pos2[2]));
                return [minPos, maxPos];
            }
            getShuffledJigsawBlocks(rotation, random) {
                const blocks = this.template.value().getBlocks().filter(block => block.state.getName().equals(SinlgePoolElement.JIGSAW_ID));
                blocks.forEach(block => block.pos = Structure.transform(block.pos, rotation, BlockPos.ZERO)); // TODO? Rotate state
                shuffle(blocks, random);
                return blocks;
            }
            toString() {
                return `[Single Pool Element: ${this.template.key()}]`;
            }
        }
        StructurePoolElement.SinlgePoolElement = SinlgePoolElement;
        class ListPoolElement extends StructurePoolElement {
            elements;
            constructor(elements) {
                super();
                this.elements = elements;
            }
            getBoundingBox(pos, rotation) {
                var minPos = undefined;
                var maxPos = undefined;
                for (const element of this.elements) {
                    const elementBoundingBox = element.getBoundingBox(pos, rotation);
                    if (!minPos || !maxPos) {
                        minPos = elementBoundingBox[0];
                        maxPos = elementBoundingBox[1];
                    }
                    else {
                        minPos[0] = Math.min(minPos[0], elementBoundingBox[0][0]);
                        minPos[1] = Math.min(minPos[1], elementBoundingBox[0][1]);
                        minPos[2] = Math.min(minPos[2], elementBoundingBox[0][2]);
                        maxPos[0] = Math.min(minPos[0], elementBoundingBox[1][0]);
                        maxPos[1] = Math.min(minPos[1], elementBoundingBox[1][1]);
                        maxPos[2] = Math.min(minPos[2], elementBoundingBox[1][2]);
                    }
                }
                return [minPos, maxPos];
            }
            getShuffledJigsawBlocks(rotation, random) {
                return this.elements[0].getShuffledJigsawBlocks(rotation, random);
            }
            toString() {
                return `[List Pool Element: ${'; '.concat(...this.elements.map(e => e.toString()))}]`;
            }
        }
        StructurePoolElement.ListPoolElement = ListPoolElement;
    })(StructurePoolElement || (StructurePoolElement = {}));

    class StructureTemplatePool {
        rawTemplates;
        fallback;
        static REGISTRY = Registry.createAndRegister('worldgen/template_pool', StructureTemplatePool.fromJson);
        totalWeight;
        constructor(rawTemplates, fallback) {
            this.rawTemplates = rawTemplates;
            this.fallback = fallback;
            this.totalWeight = rawTemplates.reduce((v, e) => v + e.weight, 0);
        }
        static structurePoolParser = Holder.parser(StructureTemplatePool.REGISTRY, StructureTemplatePool.fromJson);
        static fromJson(obj) {
            const root = Json.readObject(obj) ?? {};
            const fallback = StructureTemplatePool.structurePoolParser(root.fallback ?? '');
            const elements = Json.readArray(root.elements, (obj) => {
                const root = Json.readObject(obj) ?? {};
                const element = StructurePoolElement.fromJson(root.element);
                const weight = Json.readInt(root.weight) ?? 1;
                return { element, weight };
            }) ?? [];
            return new StructureTemplatePool(elements, fallback);
        }
        getRandomTemplate(random) {
            var v = random.nextInt(this.totalWeight);
            for (const entry of this.rawTemplates) {
                v -= entry.weight;
                if (v < 0) {
                    return entry.element;
                }
            }
            return this.rawTemplates[this.rawTemplates.length - 1].element;
        }
    }

    class WorldgenStructure {
        settings;
        constructor(settings) {
            this.settings = settings;
        }
        onTopOfChunkCenter(context, chunkX, chunkZ, heightmap = 'WORLD_SURFACE_WG') {
            const posX = (chunkX << 4) + 8;
            const posZ = (chunkZ << 4) + 8;
            return [posX, context.chunkGenerator.getBaseHeight(posX, posZ, heightmap, context.randomState) - 1, posZ]; // TODO
        }
        getLowestY(context, minX, minZ, width, depth) {
            return Math.min(context.chunkGenerator.getBaseHeight(minX, minZ, 'WORLD_SURFACE_WG', context.randomState) - 1, context.chunkGenerator.getBaseHeight(minX, minZ + depth, 'WORLD_SURFACE_WG', context.randomState) - 1, context.chunkGenerator.getBaseHeight(minX + width, minZ, 'WORLD_SURFACE_WG', context.randomState) - 1, context.chunkGenerator.getBaseHeight(minX + width, minZ + depth, 'WORLD_SURFACE_WG', context.randomState) - 1);
        }
        getLowestYIn5by5BoxOffset7Blocks(context, chunkX, chunkZ, rotation) {
            let width = 5;
            let depth = 5;
            if (rotation === Rotation.CLOCKWISE_90) {
                width = -5;
            }
            else if (rotation === Rotation.CLOCKWISE_180) {
                width = -5;
                depth = -5;
            }
            else if (rotation === Rotation.COUNTERCLOCKWISE_90) {
                depth = -5;
            }
            const posX = (chunkX << 4) + 7;
            const posZ = (chunkZ << 4) + 7;
            return BlockPos.create(posX, this.getLowestY(context, posX, posZ, width, depth), posZ);
        }
        tryGenerate(chunkX, chunkZ, context) {
            const random = LegacyRandom.fromLargeFeatureSeed(context.seed, chunkX, chunkZ);
            const pos = this.findGenerationPoint(chunkX, chunkZ, random, context);
            if (pos === undefined)
                return undefined;
            const biome = context.biomeSource.getBiome(pos[0] >> 2, pos[1] >> 2, pos[2] >> 2, context.randomState.sampler);
            return [...this.settings.validBiomes.getEntries()].findIndex((b) => b.key()?.equals(biome)) >= 0 ? pos : undefined;
        }
    }
    (function (WorldgenStructure) {
        WorldgenStructure.REGISTRY = Registry.createAndRegister('worldgen/structure', fromJson);
        class StructureSettings {
            validBiomes;
            constructor(validBiomes) {
                this.validBiomes = validBiomes;
            }
        }
        WorldgenStructure.StructureSettings = StructureSettings;
        class GenerationContext {
            seed;
            biomeSource;
            settings;
            levelHeight;
            chunkGenerator;
            randomState;
            constructor(seed, biomeSource, settings, levelHeight) {
                this.seed = seed;
                this.biomeSource = biomeSource;
                this.settings = settings;
                this.levelHeight = levelHeight;
                this.randomState = new RandomState(settings, seed);
                this.chunkGenerator = new NoiseChunkGenerator(biomeSource, settings);
            }
        }
        WorldgenStructure.GenerationContext = GenerationContext;
        const structurePoolParser = Holder.parser(StructureTemplatePool.REGISTRY, StructureTemplatePool.fromJson);
        function fromJson(obj) {
            const BiomeTagParser = HolderSet.parser(WorldgenRegistries.BIOME);
            const root = Json.readObject(obj) ?? {};
            const biomes = BiomeTagParser(root.biomes);
            const settings = new StructureSettings(biomes.value());
            switch (Json.readString(root.type)?.replace(/^minecraft:/, '')) {
                case 'buried_treasure':
                    return new BuriedTreasureStructure(settings);
                case 'desert_pyramid':
                    return new DesertPyramidStructure(settings);
                case 'end_city':
                    return new EndCityStructure(settings);
                case 'fortress':
                    return new NetherFortressStructure(settings);
                case 'igloo':
                    return new IglooStructure(settings);
                case 'jigsaw':
                    const startHeight = HeightProvider.fromJson(root.start_height);
                    const startPool = structurePoolParser(root.start_pool);
                    const startJigsawNameString = Json.readString(root.start_jigsaw_name);
                    const startJigsawName = startJigsawNameString ? Identifier.parse(startJigsawNameString) : undefined;
                    const heightmap = Heightmap.fromJson(root.project_start_to_heightmap);
                    const dimensionPadding = JigsawStructure.DimensionPadding.fromJson(root.dimension_padding);
                    return new JigsawStructure(settings, startPool, startHeight, heightmap, startJigsawName, dimensionPadding);
                case 'jungle_temple':
                    return new JungleTempleStructure(settings);
                case 'mineshaft':
                    const type = Json.readString(root.mineshaft_type) === 'mesa' ? 'mesa' : 'normal';
                    return new MineshaftStructure(settings, type);
                case 'nether_fossil':
                    return new NetherFortressStructure(settings);
                case 'ocean_monument':
                    return new OceanMonumentStructure(settings);
                case 'ocean_ruin':
                    return new OceanRuinStructure(settings);
                case 'ruined_portal':
                    return new RuinedPortalStructure(settings);
                case 'shipwreck':
                    const isBeached = Json.readBoolean(root.is_beached) ?? false;
                    return new ShipwreckStructure(settings, isBeached);
                case 'stronghold':
                    return new StrongholdStructure(settings);
                case 'swamp_hut':
                    return new SwampHutStructure(settings);
                case 'woodland_mansion':
                    return new WoodlandMansionStructure(settings);
            }
            return new BuriedTreasureStructure(settings);
        }
        WorldgenStructure.fromJson = fromJson;
        class JigsawStructure extends WorldgenStructure {
            startingPoolHolder;
            startHeight;
            projectStartToHeightmap;
            startJigsawName;
            dimensionPadding;
            constructor(settings, startingPoolHolder, startHeight, projectStartToHeightmap, startJigsawName, dimensionPadding) {
                super(settings);
                this.startingPoolHolder = startingPoolHolder;
                this.startHeight = startHeight;
                this.projectStartToHeightmap = projectStartToHeightmap;
                this.startJigsawName = startJigsawName;
                this.dimensionPadding = dimensionPadding;
            }
            findGenerationPoint(chunkX, chunkZ, random, context) {
                var y = this.startHeight(random, context.settings.noise);
                const pos = BlockPos.create(chunkX << 4, y, chunkZ << 4);
                const rotation = Rotation.getRandom(random);
                const startingPool = this.startingPoolHolder.value();
                const startingElement = startingPool.getRandomTemplate(random);
                if (startingElement instanceof StructurePoolElement.EmptyPoolElement) {
                    return undefined;
                }
                else {
                    var startJigsawOffset;
                    if (this.startJigsawName) {
                        const offset = JigsawStructure.getRandomNamedJigsaw(startingElement, this.startJigsawName, rotation, random);
                        if (offset === undefined) {
                            return undefined;
                        }
                        startJigsawOffset = offset;
                    }
                    else {
                        startJigsawOffset = BlockPos.ZERO;
                    }
                    const templateStartPos = BlockPos.subtract(pos, startJigsawOffset);
                    const boundingBox = startingElement.getBoundingBox(templateStartPos, rotation);
                    const x = ((boundingBox[1][0] + boundingBox[0][0]) / 2) ^ 0;
                    const z = ((boundingBox[1][2] + boundingBox[0][2]) / 2) ^ 0;
                    var y;
                    if (this.projectStartToHeightmap) {
                        y = pos[1] + context.chunkGenerator.getBaseHeight(x, z, this.projectStartToHeightmap, context.randomState);
                    }
                    else {
                        y = templateStartPos[1];
                    }
                    boundingBox.forEach(pos => pos[1] += y - boundingBox[0][1] - 1);
                    if (JigsawStructure.isStartTooCloseToWorldHeightLimits(this.dimensionPadding, boundingBox, context.levelHeight)) {
                        return undefined;
                    }
                    const generationPoint = BlockPos.create(x, y + startJigsawOffset[1], z);
                    //console.log(`Generating Jigsaw Structure in Chunk ${chunkX}, ${chunkZ}: rotation: ${rotation}, startingElement: ${startingElement.toString()}, center: ${x}, ${y}, ${z}`)
                    return generationPoint;
                }
            }
            static isStartTooCloseToWorldHeightLimits(dimensionPadding, boundingBox, levelHeight) {
                if (dimensionPadding === JigsawStructure.DimensionPadding.ZERO) { // reference comparison here is correct (i.e. matching vanilla), see MC-278259
                    return false;
                }
                const bottomLimit = levelHeight.minY + dimensionPadding.bottom;
                const topLimit = levelHeight.minY + levelHeight.height - dimensionPadding.top;
                return boundingBox[0][1] < bottomLimit || boundingBox[1][1] > topLimit;
            }
            static getRandomNamedJigsaw(element, name, rotation, random) {
                const jigsaws = element.getShuffledJigsawBlocks(rotation, random);
                for (const jigsaw of jigsaws) {
                    if (Identifier.parse(jigsaw.nbt?.getString('name') ?? 'minecraft:empty').equals(name)) {
                        return jigsaw.pos;
                    }
                }
                return undefined;
            }
        }
        WorldgenStructure.JigsawStructure = JigsawStructure;
        (function (JigsawStructure) {
            class DimensionPadding {
                top;
                bottom;
                static ZERO = new DimensionPadding(0, 0);
                constructor(top, bottom) {
                    this.top = top;
                    this.bottom = bottom;
                }
                static fromJson(obj) {
                    if (obj === undefined) {
                        return DimensionPadding.ZERO;
                    }
                    if (typeof obj === 'number') {
                        return new DimensionPadding(obj, obj);
                    }
                    const padding = Json.readObject(obj) ?? {};
                    return new DimensionPadding(Json.readInt(padding.top) ?? 0, Json.readInt(padding.bottom) ?? 0);
                }
            }
            JigsawStructure.DimensionPadding = DimensionPadding;
        })(JigsawStructure = WorldgenStructure.JigsawStructure || (WorldgenStructure.JigsawStructure = {}));
        class BuriedTreasureStructure extends WorldgenStructure {
            findGenerationPoint(chunkX, chunkZ, _, context) {
                return this.onTopOfChunkCenter(context, chunkX, chunkZ, 'OCEAN_FLOOR_WG');
            }
        }
        WorldgenStructure.BuriedTreasureStructure = BuriedTreasureStructure;
        class SinglePieceStructure extends WorldgenStructure {
            width;
            depth;
            constructor(settings, width, depth) {
                super(settings);
                this.width = width;
                this.depth = depth;
            }
            findGenerationPoint(chunkX, chunkZ, _, context) {
                if (this.getLowestY(context, chunkX << 4, chunkZ << 4, this.width, this.depth) < context.settings.seaLevel) {
                    return undefined;
                }
                else {
                    return this.onTopOfChunkCenter(context, chunkX, chunkZ);
                }
            }
        }
        class DesertPyramidStructure extends SinglePieceStructure {
            constructor(settings) {
                super(settings, 21, 21);
            }
        }
        WorldgenStructure.DesertPyramidStructure = DesertPyramidStructure;
        class EndCityStructure extends WorldgenStructure {
            findGenerationPoint(chunkX, chunkZ, random, context) {
                const rotation = Rotation.getRandom(random);
                const pos = this.getLowestYIn5by5BoxOffset7Blocks(context, chunkX, chunkZ, rotation);
                if (pos[1] < 60)
                    return undefined;
                return pos;
            }
        }
        WorldgenStructure.EndCityStructure = EndCityStructure;
        class NetherFortressStructure extends WorldgenStructure {
            findGenerationPoint(chunkX, chunkZ) {
                return BlockPos.create(chunkX << 4, 64, chunkZ << 4);
            }
        }
        WorldgenStructure.NetherFortressStructure = NetherFortressStructure;
        class IglooStructure extends WorldgenStructure {
            findGenerationPoint(chunkX, chunkZ, _, context) {
                return this.onTopOfChunkCenter(context, chunkX, chunkZ);
            }
        }
        WorldgenStructure.IglooStructure = IglooStructure;
        class JungleTempleStructure extends SinglePieceStructure {
            constructor(settings) {
                super(settings, 12, 15);
            }
        }
        WorldgenStructure.JungleTempleStructure = JungleTempleStructure;
        class MineshaftStructure extends WorldgenStructure {
            type;
            constructor(settings, type) {
                super(settings);
                this.type = type;
            }
            findGenerationPoint(chunkX, chunkZ, random, context) {
                throw new Error('Method not implemented.');
            }
        }
        WorldgenStructure.MineshaftStructure = MineshaftStructure;
        class NetherFossilStructure extends WorldgenStructure {
            height;
            constructor(settings, height) {
                super(settings);
                this.height = height;
            }
            findGenerationPoint(chunkX, chunkZ) {
                throw new Error('Method not implemented.');
            }
        }
        WorldgenStructure.NetherFossilStructure = NetherFossilStructure;
        class OceanMonumentStructure extends WorldgenStructure {
            findGenerationPoint(chunkX, chunkZ) {
                throw new Error('Method not implemented.');
            }
        }
        WorldgenStructure.OceanMonumentStructure = OceanMonumentStructure;
        class OceanRuinStructure extends WorldgenStructure {
            findGenerationPoint(chunkX, chunkZ, _, context) {
                return this.onTopOfChunkCenter(context, chunkX, chunkZ, 'OCEAN_FLOOR_WG');
            }
        }
        WorldgenStructure.OceanRuinStructure = OceanRuinStructure;
        class RuinedPortalStructure extends WorldgenStructure {
            findGenerationPoint(chunkX, chunkZ) {
                throw new Error('Method not implemented.');
            }
        }
        WorldgenStructure.RuinedPortalStructure = RuinedPortalStructure;
        class ShipwreckStructure extends WorldgenStructure {
            isBeached;
            constructor(settings, isBeached) {
                super(settings);
                this.isBeached = isBeached;
            }
            findGenerationPoint(chunkX, chunkZ, _, context) {
                return this.onTopOfChunkCenter(context, chunkX, chunkZ, this.isBeached ? 'WORLD_SURFACE_WG' : 'OCEAN_FLOOR_WG');
            }
        }
        WorldgenStructure.ShipwreckStructure = ShipwreckStructure;
        class StrongholdStructure extends WorldgenStructure {
            findGenerationPoint(chunkX, chunkZ) {
                return BlockPos.create(chunkX << 4, 0, chunkZ << 4);
            }
        }
        WorldgenStructure.StrongholdStructure = StrongholdStructure;
        class SwampHutStructure extends WorldgenStructure {
            findGenerationPoint(chunkX, chunkZ, _, context) {
                return this.onTopOfChunkCenter(context, chunkX, chunkZ);
            }
        }
        WorldgenStructure.SwampHutStructure = SwampHutStructure;
        class WoodlandMansionStructure extends WorldgenStructure {
            findGenerationPoint(chunkX, chunkZ, random, context) {
                const rotation = Rotation.getRandom(random);
                const pos = this.getLowestYIn5by5BoxOffset7Blocks(context, chunkX, chunkZ, rotation);
                if (pos[1] < 60)
                    return undefined;
                return pos;
            }
        }
        WorldgenStructure.WoodlandMansionStructure = WoodlandMansionStructure;
    })(WorldgenStructure || (WorldgenStructure = {}));

    class StructureSet {
        structures;
        placement;
        static REGISTRY = Registry.createAndRegister('worldgen/structure_set', StructureSet.fromJson);
        constructor(structures, placement) {
            this.structures = structures;
            this.placement = placement;
        }
        static fromJson(obj) {
            const root = Json.readObject(obj) ?? {};
            const structures = Json.readArray(root.structures, (StructureSet.StructureSelectionEntry.fromJson)) ?? [];
            const placement = StructurePlacement.fromJson(root.placement);
            return new StructureSet(structures, placement);
        }
        getStructureInChunk(chunkX, chunkZ, context) {
            this.placement.prepare(context.biomeSource, context.randomState.sampler, context.seed);
            if (!this.placement.isStructureChunk(context.seed, chunkX, chunkZ)) {
                return undefined;
            }
            if (this.structures.length === 0)
                return undefined;
            if (this.structures.length === 1) {
                const pos = this.structures[0].structure.value().tryGenerate(chunkX, chunkZ, context);
                if (pos !== undefined) {
                    return { id: this.structures[0].structure.key(), pos };
                }
            }
            else {
                const random = LegacyRandom.fromLargeFeatureSeed(context.seed, chunkX, chunkZ);
                const list = Object.assign([], this.structures);
                let totalWeight = list.reduce((v, e, i) => v + e.weight, 0);
                while (list.length > 0) {
                    let weightIndex = random.nextInt(totalWeight);
                    let id;
                    let entry;
                    for ([id, entry] of list.entries()) {
                        weightIndex -= entry.weight;
                        if (weightIndex < 0) {
                            break;
                        }
                    }
                    const pos = entry.structure.value().tryGenerate(chunkX, chunkZ, context);
                    if (pos !== undefined) {
                        return { id: entry.structure.key(), pos };
                    }
                    list.splice(id, 1);
                    totalWeight -= entry.weight;
                }
            }
            return undefined;
        }
    }
    (function (StructureSet) {
        class StructureSelectionEntry {
            structure;
            weight;
            constructor(structure, weight) {
                this.structure = structure;
                this.weight = weight;
            }
            static fromJson(obj) {
                const root = Json.readObject(obj) ?? {};
                return new StructureSelectionEntry(Holder.reference(WorldgenStructure.REGISTRY, Identifier.parse(Json.readString(root.structure) ?? 'minecraft:empty')), Json.readInt(root.weight) ?? 1);
            }
        }
        StructureSet.StructureSelectionEntry = StructureSelectionEntry;
    })(StructureSet || (StructureSet = {}));

    class StructurePlacement {
        locateOffset;
        frequencyReductionMethod;
        frequency;
        salt;
        exclusionZone;
        constructor(locateOffset, frequencyReductionMethod, frequency, salt, exclusionZone) {
            this.locateOffset = locateOffset;
            this.frequencyReductionMethod = frequencyReductionMethod;
            this.frequency = frequency;
            this.salt = salt;
            this.exclusionZone = exclusionZone;
        }
        static fromJson(obj) {
            const root = Json.readObject(obj) ?? {};
            const type = Json.readString(root.type)?.replace(/^minecraft:/, '');
            const locateOffset = BlockPos.fromJson(root.locate_offset);
            const frequencyReductionMethod = StructurePlacement.FrequencyReducer.fromType(Json.readString(root.frequency_reduction_method) ?? 'default');
            const frequency = Json.readNumber(root.frequency) ?? 1;
            const salt = Json.readInt(root.salt) ?? 0;
            const exclusionZone = 'exclusion_zone' in root ? StructurePlacement.ExclusionZone.fromJson(root.exclusion_zone) : undefined;
            switch (type) {
                case 'random_spread':
                    const spacing = Json.readInt(root.spacing) ?? 1;
                    const separation = Json.readInt(root.separation) ?? 1;
                    const spreadType = StructurePlacement.SpreadType.fromJson(root.spread_type);
                    return new StructurePlacement.RandomSpreadStructurePlacement(locateOffset, frequencyReductionMethod, frequency, salt, exclusionZone, spacing, separation, spreadType);
                case 'concentric_rings':
                    const distance = Json.readInt(root.distance) ?? 1;
                    const spread = Json.readInt(root.spread) ?? 1;
                    const count = Json.readInt(root.count) ?? 1;
                    const preferredBiomes = HolderSet.parser(WorldgenRegistries.BIOME)(root.preferred_biomes);
                    return new StructurePlacement.ConcentricRingsStructurePlacement(locateOffset, frequencyReductionMethod, frequency, salt, exclusionZone, distance, spread, count, preferredBiomes);
            }
            return new StructurePlacement.RandomSpreadStructurePlacement([0, 0, 0], StructurePlacement.FrequencyReducer.ProbabilityReducer, 1, 0, undefined, 1, 1, 'linear');
        }
        isStructureChunk(seed, chunkX, chunkZ) {
            if (!this.isPlacementChunk(seed, chunkX, chunkZ)) {
                return false;
            }
            else if (this.frequency < 1.0 && !this.frequencyReductionMethod(seed, this.salt, chunkX, chunkZ, this.frequency)) {
                return false;
            }
            else if (this.exclusionZone && this.exclusionZone.isPlacementForbidden(seed, chunkX, chunkZ)) {
                return false;
            }
            else {
                return true;
            }
        }
        prepare(_biomeSource, _sampler, _concentricRingsSeed) { }
    }
    (function (StructurePlacement) {
        (function (FrequencyReducer) {
            function fromType(type) {
                switch (type) {
                    case 'legacy_type_1': return LegacyPillagerOutpostReducer;
                    case 'legacy_type_2': return LegacyArbitrarySaltProbabilityReducer;
                    case 'legacy_type_3': return LegacyProbabilityReducerWithDouble;
                    case 'default':
                    default: return ProbabilityReducer;
                }
            }
            FrequencyReducer.fromType = fromType;
            function ProbabilityReducer(seed, salt, chunkX, chunkZ, frequency) {
                const random = LegacyRandom.fromLargeFeatureWithSalt(seed, salt, chunkX, chunkZ); // [sic]
                return random.nextFloat() < frequency;
            }
            FrequencyReducer.ProbabilityReducer = ProbabilityReducer;
            function LegacyProbabilityReducerWithDouble(seed, _, chunkX, chunkZ, frequency) {
                const random = LegacyRandom.fromLargeFeatureSeed(seed, chunkX, chunkZ);
                return random.nextDouble() < frequency;
            }
            FrequencyReducer.LegacyProbabilityReducerWithDouble = LegacyProbabilityReducerWithDouble;
            function LegacyArbitrarySaltProbabilityReducer(seed, _, chunkX, chunkZ, frequency) {
                const random = LegacyRandom.fromLargeFeatureWithSalt(seed, chunkX, chunkZ, 10387320);
                return random.nextFloat() < frequency;
            }
            FrequencyReducer.LegacyArbitrarySaltProbabilityReducer = LegacyArbitrarySaltProbabilityReducer;
            function LegacyPillagerOutpostReducer(seed, _, chunkX, chunkZ, frequency) {
                const a = chunkX >> 4;
                const b = chunkZ >> 4;
                const random = new LegacyRandom(BigInt(a ^ b << 4) ^ seed);
                random.nextInt();
                return random.nextInt(Math.floor(1 / frequency)) === 0;
            }
            FrequencyReducer.LegacyPillagerOutpostReducer = LegacyPillagerOutpostReducer;
        })(StructurePlacement.FrequencyReducer || (StructurePlacement.FrequencyReducer = {}));
        class ExclusionZone {
            otherSet;
            chunkCount;
            constructor(otherSet, chunkCount) {
                this.otherSet = otherSet;
                this.chunkCount = chunkCount;
            }
            static fromJson(obj) {
                const root = Json.readObject(obj) ?? {};
                return new ExclusionZone(Holder.reference(StructureSet.REGISTRY, Identifier.parse(Json.readString(root.other_set) ?? '')), Json.readInt(root.chunk_count) ?? 1);
            }
            isPlacementForbidden(seed, chunkX, chunkZ) {
                const placement = this.otherSet.value().placement;
                return placement
                    .getPotentialStructureChunks(seed, chunkX - this.chunkCount, chunkZ - this.chunkCount, chunkX + this.chunkCount, chunkZ + this.chunkCount)
                    .findIndex((chunk) => Math.abs(chunk[0] - chunkX) <= this.chunkCount && Math.abs(chunk[1] - chunkZ) <= this.chunkCount && placement.isStructureChunk(seed, chunk[0], chunk[1])) >= 0;
            }
        }
        StructurePlacement.ExclusionZone = ExclusionZone;
        (function (SpreadType) {
            function fromJson(obj) {
                const string = Json.readString(obj) ?? 'linear';
                if (string === 'triangular')
                    return 'triangular';
                return 'linear';
            }
            SpreadType.fromJson = fromJson;
        })(StructurePlacement.SpreadType || (StructurePlacement.SpreadType = {}));
        class RandomSpreadStructurePlacement extends StructurePlacement {
            spacing;
            separation;
            spreadType;
            constructor(locateOffset, frequencyReductionMethod, frequency, salt, exclusionZone, spacing, separation, spreadType) {
                super(locateOffset, frequencyReductionMethod, frequency, salt, exclusionZone);
                this.spacing = spacing;
                this.separation = separation;
                this.spreadType = spreadType;
            }
            evaluateSpread(random, max) {
                switch (this.spreadType) {
                    case 'linear':
                        return random.nextInt(max);
                    case 'triangular':
                        return Math.floor((random.nextInt(max) + random.nextInt(max)) / 2);
                }
            }
            getPotentialStructureChunk(seed, chunkX, chunkZ) {
                const x = Math.floor(chunkX / this.spacing);
                const z = Math.floor(chunkZ / this.spacing);
                const random = LegacyRandom.fromLargeFeatureWithSalt(seed, x, z, this.salt);
                const maxOffset = this.spacing - this.separation;
                const offsetX = this.evaluateSpread(random, maxOffset);
                const offsetZ = this.evaluateSpread(random, maxOffset);
                return [x * this.spacing + offsetX, z * this.spacing + offsetZ];
            }
            isPlacementChunk(seed, chunkX, chunkZ) {
                const [placementX, palcementZ] = this.getPotentialStructureChunk(seed, chunkX, chunkZ);
                return placementX === chunkX && palcementZ === chunkZ;
            }
            getPotentialStructureChunks(seed, minChunkX, minChunkZ, maxChunkX, maxChunkZ) {
                const positions = [];
                for (let chunkX = Math.floor(minChunkX / this.spacing) * this.spacing; chunkX <= maxChunkX; chunkX += this.spacing) {
                    for (let chunkZ = Math.floor(minChunkZ / this.spacing) * this.spacing; chunkZ <= maxChunkZ; chunkZ += this.spacing) {
                        positions.push(this.getPotentialStructureChunk(seed, chunkX, chunkZ));
                    }
                }
                return positions;
            }
        }
        StructurePlacement.RandomSpreadStructurePlacement = RandomSpreadStructurePlacement;
        const SEARCH_RANGE = 112;
        class ConcentricRingsStructurePlacement extends StructurePlacement {
            distance;
            spread;
            count;
            preferredBiomes;
            positions;
            constructor(locateOffset, frequencyReductionMethod, frequency, salt, exclusionZone, distance, spread, count, preferredBiomes) {
                super(locateOffset, frequencyReductionMethod, frequency, salt, exclusionZone);
                this.distance = distance;
                this.spread = spread;
                this.count = count;
                this.preferredBiomes = preferredBiomes;
            }
            prepare(biomeSource, sampler, concentricRingsSeed) {
                if (this.positions !== undefined) {
                    return;
                }
                this.positions = [];
                if (this.count === 0) {
                    return;
                }
                const random = new LegacyRandom(concentricRingsSeed);
                var angle = random.nextDouble() * Math.PI * 2;
                var current_spread = this.spread;
                var ringNr = 0;
                var posInRingNr = 0;
                const preferredBiomes = [...this.preferredBiomes.value().getEntries()].flatMap(b => b.key() ?? []);
                for (var i = 0; i < this.count; i++) {
                    const current_distance = 4 * this.distance + this.distance * ringNr * 6 + (random.nextDouble() - 0.5) * this.distance * 2.5;
                    const chunkX = Math.round(Math.cos(angle) * current_distance);
                    const chunkZ = Math.round(Math.sin(angle) * current_distance);
                    const posX = (chunkX << 4) + 8;
                    const posZ = (chunkZ << 4) + 8;
                    const forkedRandom = random.fork();
                    const provider = () => {
                        const searchResult = BiomeSource.findBiomeHorizontal(biomeSource, posX, 0, posZ, SEARCH_RANGE, (biome) => preferredBiomes.findIndex(b => b.equals(biome)) >= 0, forkedRandom, sampler);
                        if (searchResult) {
                            return [searchResult.pos[0] >> 4, searchResult.pos[2] >> 4];
                        }
                        else {
                            return [chunkX, chunkZ];
                        }
                    };
                    this.positions.push({ center: [chunkX, chunkZ], real: provider });
                    angle += Math.PI * 2 / current_spread;
                    posInRingNr++;
                    if (posInRingNr == current_spread) {
                        ringNr++;
                        posInRingNr = 0;
                        current_spread += 2 * current_spread / (ringNr + 1);
                        current_spread = Math.min(current_spread, this.count - i);
                        angle += random.nextDouble() * Math.PI * 2;
                    }
                }
            }
            isPlacementChunk(seed, chunkX, chunkZ) {
                if (this.positions === undefined) {
                    console.warn('trying to access concentric rings placement before position calculation');
                    return false;
                }
                return this.getPotentialStructureChunks(seed, chunkX, chunkZ, chunkX, chunkZ).findIndex((p) => p[0] === chunkX && p[1] === chunkZ) >= 0;
            }
            getPotentialStructureChunks(seed, minChunkX, minChunkZ, maxChunkX, maxChunkZ) {
                if (this.positions === undefined) {
                    console.warn('trying to access concentric rings placement before position calculation');
                    return [];
                }
                const results = [];
                for (const position of this.positions) {
                    if (position.center[0] < minChunkX - (SEARCH_RANGE >> 4))
                        continue;
                    if (position.center[0] > maxChunkX + (SEARCH_RANGE >> 4))
                        continue;
                    if (position.center[1] < minChunkZ - (SEARCH_RANGE >> 4))
                        continue;
                    if (position.center[1] > maxChunkZ + (SEARCH_RANGE >> 4))
                        continue;
                    if (position.real instanceof Function) {
                        position.real = position.real();
                    }
                    results.push(position.real);
                }
                return results;
            }
        }
        StructurePlacement.ConcentricRingsStructurePlacement = ConcentricRingsStructurePlacement;
    })(StructurePlacement || (StructurePlacement = {}));

    class NbtPath {
        constructor(arr = []) {
            this.arr = arr;
        }
        pop(count = 1) {
            if (count === 0)
                return new NbtPath(this.arr);
            return new NbtPath(this.arr.slice(0, -count));
        }
        shift(count = 1) {
            return new NbtPath(this.arr.slice(count));
        }
        push(...el) {
            return new NbtPath([...this.arr, ...el]);
        }
        head() {
            return this.arr[0];
        }
        last() {
            return this.arr[this.arr.length - 1];
        }
        length() {
            return this.arr.length;
        }
        startsWith(other) {
            return other.arr.every((e, i) => this.arr[i] === e);
        }
        subPaths() {
            return [...Array(this.arr.length + 1)].map((_, i) => this.pop(this.arr.length - i));
        }
        equals(other) {
            return other.length() === this.length()
                && other.arr.every((e, i) => this.arr[i] === e);
        }
        toString() {
            return this.arr
                .map(e => (typeof e === 'string') ? `.${e}` : `[${e}]`)
                .join('')
                .replace(/^\./, '');
        }
    }

    function mapEdit(edit, mapper) {
        switch (edit.type) {
            case 'composite': return Object.assign(Object.assign({}, edit), { edits: edit.edits.map(e => mapEdit(e, mapper)) });
            case 'chunk': return Object.assign(Object.assign({}, edit), { edit: mapEdit(edit.edit, mapper) });
            default: return mapper(edit);
        }
    }
    function applyEdit(file, edit, logger) {
        logger === null || logger === void 0 ? void 0 : logger.info(`Applying edit to file ${editToString(edit)}`);
        if (file instanceof NbtRegion || file instanceof NbtRegion.Ref) {
            if (edit.type !== 'chunk') {
                throw new Error(`Expected chunk edit, but got '${edit.type}'`);
            }
            const chunk = file.findChunk(edit.x, edit.z);
            const chunkFile = chunk === null || chunk === void 0 ? void 0 : chunk.getFile();
            if (chunkFile === undefined) {
                // chunk does not exist or the ref is not loaded, so no need to apply any edits.
                logger === null || logger === void 0 ? void 0 : logger.error(`Cannot apply chunk edit, chunk x=${edit.x} z=${edit.z} is not loaded or does not exist`);
                return;
            }
            applyEdit(chunkFile, edit.edit, logger);
            if (chunk instanceof NbtChunk) {
                chunk.markDirty();
            }
        }
        else {
            if (edit.type === 'chunk') {
                throw new Error('Cannot apply chunk edit, this is not a region file');
            }
            if (edit.type !== 'composite' && edit.path.length === 0) {
                if (edit.type !== 'set') {
                    throw new Error(`Cannot apply ${edit.type} edit on the root, expected 'set'`);
                }
                const newTag = NbtTag.fromJsonWithId(edit.new);
                if (!newTag.isCompound()) {
                    throw new Error(`Expected a compound, but got ${NbtType[newTag.getId()]}`);
                }
                file.root = newTag;
            }
            else {
                applyEditTag(file.root, edit, logger);
            }
        }
    }
    function getEditedFile(file, edit) {
        if (file instanceof NbtRegion || file instanceof NbtRegion.Ref) {
            if (edit.type !== 'chunk') {
                throw new Error(`Expected chunk edit, but got '${edit.type}'`);
            }
            const chunk = file.findChunk(edit.x, edit.z);
            return { file: chunk === null || chunk === void 0 ? void 0 : chunk.getFile(), edit: edit.edit };
        }
        else {
            return { file, edit };
        }
    }
    function applyEditTag(tag, edit, logger) {
        logger === null || logger === void 0 ? void 0 : logger.info(`Applying edit ${editToString(edit)}`);
        try {
            if (edit.type === 'composite') {
                edit.edits.forEach(edit => applyEditTag(tag, edit, logger));
                return;
            }
            else if (edit.type === 'chunk') {
                throw new Error('Cannot apply chunk edit to a tag');
            }
            if (edit.path.length === 0) {
                throw new Error('Cannot apply edit to the root');
            }
            const path = new NbtPath(edit.path);
            const node = getNode(tag, path.pop());
            const last = path.last();
            switch (edit.type) {
                case 'set': return setValue(node, last, NbtTag.fromJsonWithId(edit.new));
                case 'add': return addValue(node, last, NbtTag.fromJsonWithId(edit.value));
                case 'remove': return removeValue(node, last);
                case 'move': {
                    if (edit.source.length === 0) {
                        throw new Error('Cannot move the root');
                    }
                    const sPath = new NbtPath(edit.source);
                    const sNode = getNode(tag, sPath.pop());
                    const sLast = sPath.last();
                    return moveNode(node, last, sNode, sLast);
                }
            }
        }
        catch (e) {
            logger === null || logger === void 0 ? void 0 : logger.error(`Error applying edit to tag: ${e.message}`);
            throw e;
        }
    }
    function editToString(edit) {
        return `type=${edit.type} ${edit.type === 'chunk' ? `x=${edit.x} z=${edit.z} ` : ''}${edit.type !== 'composite' && edit.type !== 'chunk' ? ` path=${new NbtPath(edit.path).toString()}` : ''} ${edit.type === 'remove' || edit.type === 'composite' || edit.type === 'chunk' ? '' : edit.type === 'move' ? `source=${new NbtPath(edit.source).toString()}` : `value=${(a => a.slice(0, 40) + (a.length > 40 ? '...' : ''))(JSON.stringify(edit.type === 'set' ? edit.new : edit.value))}`}`;
    }
    function getNode(tag, path) {
        let node = tag;
        for (const el of path.arr) {
            if ((node === null || node === void 0 ? void 0 : node.isCompound()) && typeof el === 'string') {
                node = node.get(el);
            }
            else if ((node === null || node === void 0 ? void 0 : node.isListOrArray()) && typeof el === 'number') {
                node = node.get(el);
            }
            else {
                node = undefined;
            }
            if (node === undefined) {
                throw new Error(`Invalid path ${path.toString()}`);
            }
        }
        return node;
    }
    function moveNode(tag, last, sTag, sLast) {
        const value = getNode(sTag, new NbtPath([sLast]));
        addValue(tag, last, value);
        removeValue(sTag, sLast);
    }
    function setValue(tag, last, value) {
        if (tag.isCompound() && typeof last === 'string') {
            tag.set(last, value);
        }
        else if (tag.isList() && typeof last === 'number') {
            tag.set(last, value);
        }
        else if (tag.isByteArray() && typeof last === 'number' && value.isByte()) {
            tag.set(last, value);
        }
        else if (tag.isIntArray() && typeof last === 'number' && value.isInt()) {
            tag.set(last, value);
        }
        else if (tag.isLongArray() && typeof last === 'number' && value.isLong()) {
            tag.set(last, value);
        }
    }
    function addValue(tag, last, value) {
        if (tag.isCompound() && typeof last === 'string') {
            tag.set(last, value);
        }
        else if (tag.isList() && typeof last === 'number') {
            tag.insert(last, value);
        }
        else if (tag.isByteArray() && typeof last === 'number' && value.isByte()) {
            tag.insert(last, value);
        }
        else if (tag.isIntArray() && typeof last === 'number' && value.isInt()) {
            tag.insert(last, value);
        }
        else if (tag.isLongArray() && typeof last === 'number' && value.isLong()) {
            tag.insert(last, value);
        }
    }
    function removeValue(tag, last) {
        if (tag.isCompound() && typeof last === 'string') {
            tag.delete(last);
        }
        else if (tag.isListOrArray() && typeof last === 'number') {
            tag.delete(last);
        }
    }
    function searchNodes(tag, query) {
        const results = [];
        let parsedValue = undefined;
        try {
            if (query.value !== undefined) {
                parsedValue = NbtTag.fromString(query.value);
            }
        }
        catch (e) { }
        searchNodesImpl(new NbtPath(), tag, query, results, parsedValue);
        return results;
    }
    function searchNodesImpl(path, tag, query, results, parsedValue) {
        if (matchesNode(path, tag, query, parsedValue)) {
            results.push(path);
        }
        if (tag.isCompound()) {
            [...tag.keys()].sort().forEach(k => {
                searchNodesImpl(path.push(k), tag.get(k), query, results, parsedValue);
            });
        }
        else if (tag.isListOrArray()) {
            tag.forEach((v, i) => {
                searchNodesImpl(path.push(i), v, query, results, parsedValue);
            });
        }
    }
    function matchesNode(path, tag, query, parsedValue) {
        const last = path.last();
        const typeMatches = !query.type || tag.getId() === query.type;
        const nameMatches = !query.name || (typeof last === 'string' && last.includes(query.name));
        const valueMatches = !query.value || matchesValue(tag, query.value, parsedValue);
        return typeMatches && nameMatches && valueMatches;
    }
    function matchesValue(tag, value, parsedValue) {
        if (parsedValue && tag.getId() == parsedValue.getId() && tag.toString() == parsedValue.toString()) {
            return true;
        }
        try {
            if (tag.isString()) {
                return tag.getAsString().includes(value);
            }
            else if (tag.isLong()) {
                const long = NbtLong.bigintToPair(BigInt(value));
                return tag.getAsPair()[0] === long[0] && tag.getAsPair()[1] === long[1];
            }
            else if (tag.isNumber()) {
                return tag.getAsNumber() === JSON.parse(value);
            }
        }
        catch (e) { }
        return false;
    }
    function replaceNode(tag, path, replace) {
        const edits = [];
        if (replace.value) {
            const node = getNode(tag, path);
            const newNode = parsePrimitive(node.getId(), replace.value);
            edits.push({ type: 'set', path: path.arr, old: node.toJsonWithId(), new: newNode.toJsonWithId() });
        }
        if (replace.name) {
            edits.push({ type: 'move', source: path.arr, path: path.pop().push(replace.name).arr });
        }
        if (edits.length === 1) {
            return edits[0];
        }
        return { type: 'composite', edits };
    }
    function serializePrimitive(tag) {
        if (tag.isString())
            return tag.getAsString();
        if (tag.isLong())
            return NbtLong.pairToString(tag.getAsPair());
        if (tag.isNumber())
            return tag.getAsNumber().toString();
        return '';
    }
    function parsePrimitive(id, value) {
        switch (id) {
            case NbtType.String: return new NbtString(value);
            case NbtType.Byte: return new NbtByte(parseInt(value));
            case NbtType.Short: return new NbtShort(parseInt(value));
            case NbtType.Int: return new NbtInt(parseInt(value));
            case NbtType.Long: return new NbtLong(BigInt(value));
            case NbtType.Float: return new NbtFloat(parseFloat(value));
            case NbtType.Double: return new NbtDouble(parseFloat(value));
            default: return NbtEnd.INSTANCE;
        }
    }

    const LOCALES = {
        copy: 'Copy',
        name: 'Name',
        value: 'Value',
        confirm: 'Confirm',
        addTag: 'Add Tag',
        editTag: 'Edit',
        removeTag: 'Remove',
        renameTag: 'Rename',
        grid: 'Show Grid',
        invisibleBlocks: 'Show Invisible Blocks',
        invisibleBlocksUnavailable: 'Invisible blocks is unavailable for large structures',
        'panel.structure': '3D',
        'panel.chunk': '3D',
        'panel.map': 'Map',
        'panel.region': 'Region',
        'panel.default': 'Default',
        'panel.snbt': 'SNBT',
        'panel.info': 'File Info',
    };
    function locale(key) {
        var _a;
        return (_a = LOCALES[key]) !== null && _a !== void 0 ? _a : key;
    }

    const OPAQUE_BLOCKS = new Set([
        'minecraft:acacia_planks',
        'minecraft:acacia_wood',
        'minecraft:amethyst_block',
        'minecraft:ancient_debris',
        'minecraft:andesite',
        'minecraft:barrel',
        'minecraft:bamboo_block',
        'minecraft:bamboo_mosaic',
        'minecraft:bamboo_planks',
        'minecraft:basalt',
        'minecraft:bedrock',
        'minecraft:bee_nest',
        'minecraft:beehive',
        'minecraft:birch_log',
        'minecraft:birch_planks',
        'minecraft:birch_wood',
        'minecraft:black_concrete',
        'minecraft:black_concrete_powder',
        'minecraft:black_glazed_terracotta',
        'minecraft:black_terracotta',
        'minecraft:blackstone',
        'minecraft:blast_furnace',
        'minecraft:blue_concrete',
        'minecraft:blue_concrete_powder',
        'minecraft:blue_glazed_terracotta',
        'minecraft:blue_ice',
        'minecraft:blue_terracotta',
        'minecraft:blue_wool',
        'minecraft:bone_block',
        'minecraft:bookshelf',
        'minecraft:brain_coral_block',
        'minecraft:bricks',
        'minecraft:brown_concrete',
        'minecraft:brown_concrete_powder',
        'minecraft:brown_glazed_terracotta',
        'minecraft:brown_mushroom_block',
        'minecraft:brown_terracotta',
        'minecraft:brown_wool',
        'minecraft:bubble_coral_block',
        'minecraft:calcite',
        'minecraft:cartography_table',
        'minecraft:carved_pumpkin',
        'minecraft:chain_command_block',
        'minecraft:cherry_log',
        'minecraft:cherry_planks',
        'minecraft:cherry_wood',
        'minecraft:chiseled_bookshelf',
        'minecraft:chiseled_copper',
        'minecraft:chiseled_deepslate',
        'minecraft:chiseled_nether_bricks',
        'minecraft:chiseled_polished_blackstone',
        'minecraft:chiseled_quartz_block',
        'minecraft:chiseled_red_sandstone',
        'minecraft:chiseled_resin_bricks',
        'minecraft:chiseled_sandstone',
        'minecraft:chiseled_stone_bricks',
        'minecraft:chiseled_tuff',
        'minecraft:chiseled_tuff_bricks',
        'minecraft:clay',
        'minecraft:coal_block',
        'minecraft:coal_ore',
        'minecraft:coarse_dirt',
        'minecraft:cobbled_deepslate',
        'minecraft:cobbled_deepslate_wall',
        'minecraft:cobblestone',
        'minecraft:command_block',
        'minecraft:copper_block',
        'minecraft:copper_bulb',
        'minecraft:copper_ore',
        'minecraft:cracked_deepslate_bricks',
        'minecraft:cracked_deepslate_tiles',
        'minecraft:cracked_nether_bricks',
        'minecraft:cracked_polished_blackstone_bricks',
        'minecraft:cracked_stone_bricks',
        'minecraft:crafting_table',
        'minecraft:crafter',
        'minecraft:creaking_heart',
        'minecraft:crimson_hyphae',
        'minecraft:crimson_nylium',
        'minecraft:crimson_planks',
        'minecraft:crimson_roots',
        'minecraft:crimson_stem',
        'minecraft:crying_obsidian',
        'minecraft:cut_copper',
        'minecraft:cut_red_sandstone',
        'minecraft:cut_sandstone',
        'minecraft:cyan_concrete',
        'minecraft:cyan_concrete_powder',
        'minecraft:cyan_glazed_terracotta',
        'minecraft:cyan_terracotta',
        'minecraft:cyan_wool',
        'minecraft:dark_oak_log',
        'minecraft:dark_oak_planks',
        'minecraft:dark_oak_wood',
        'minecraft:dark_prismarine',
        'minecraft:dead_brain_coral_block',
        'minecraft:dead_bubble_coral_block',
        'minecraft:dead_fire_coral_block',
        'minecraft:dead_horn_coral_block',
        'minecraft:dead_tube_coral_block',
        'minecraft:deepslate',
        'minecraft:deepslate_bricks',
        'minecraft:deepslate_coal_ore',
        'minecraft:deepslate_copper_ore',
        'minecraft:deepslate_diamond_ore',
        'minecraft:deepslate_emerald_ore',
        'minecraft:deepslate_gold_ore',
        'minecraft:deepslate_iron_ore',
        'minecraft:deepslate_lapis_ore',
        'minecraft:deepslate_redstone_ore',
        'minecraft:deepslate_tiles',
        'minecraft:diamond_block',
        'minecraft:diamond_ore',
        'minecraft:diorite',
        'minecraft:dirt',
        'minecraft:dispenser',
        'minecraft:dried_kelp_block',
        'minecraft:dripstone_block',
        'minecraft:dropper',
        'minecraft:emerald_block',
        'minecraft:emerald_ore',
        'minecraft:end_stone',
        'minecraft:end_stone_bricks',
        'minecraft:exposed_chiseled_copper',
        'minecraft:exposed_copper',
        'minecraft:exposed_copper_bulb',
        'minecraft:exposed_cut_copper',
        'minecraft:fire_coral_block',
        'minecraft:fletching_table',
        'minecraft:furnace',
        'minecraft:gilded_blackstone',
        'minecraft:glowstone',
        'minecraft:gold_block',
        'minecraft:gold_ore',
        'minecraft:granite',
        'minecraft:grass_block',
        'minecraft:gravel',
        'minecraft:gray_concrete',
        'minecraft:gray_concrete_powder',
        'minecraft:gray_glazed_terracotta',
        'minecraft:gray_terracotta',
        'minecraft:gray_wool',
        'minecraft:green_concrete',
        'minecraft:green_concrete_powder',
        'minecraft:green_glazed_terracotta',
        'minecraft:green_terracotta',
        'minecraft:green_wool',
        'minecraft:hay_block',
        'minecraft:honeycomb_block',
        'minecraft:horn_coral_block',
        'minecraft:infested_chiseled_stone_bricks',
        'minecraft:infested_cobblestone',
        'minecraft:infested_cracked_stone_bricks',
        'minecraft:infested_deepslate',
        'minecraft:infested_mossy_stone_bricks',
        'minecraft:infested_stone',
        'minecraft:infested_stone_bricks',
        'minecraft:iron_block',
        'minecraft:iron_ore',
        'minecraft:jack_o_lantern',
        'minecraft:jigsaw',
        'minecraft:jukebox',
        'minecraft:jungle_log',
        'minecraft:jungle_planks',
        'minecraft:jungle_wood',
        'minecraft:lapis_block',
        'minecraft:lapis_ore',
        'minecraft:light_blue_concrete',
        'minecraft:light_blue_concrete_powder',
        'minecraft:light_blue_glazed_terracotta',
        'minecraft:light_blue_terracotta',
        'minecraft:light_blue_wool',
        'minecraft:light_gray_concrete',
        'minecraft:light_gray_concrete_powder',
        'minecraft:light_gray_glazed_terracotta',
        'minecraft:light_gray_terracotta',
        'minecraft:light_gray_wool',
        'minecraft:lime_concrete',
        'minecraft:lime_concrete_powder',
        'minecraft:lime_glazed_terracotta',
        'minecraft:lime_terracotta',
        'minecraft:lime_wool',
        'minecraft:lodestone',
        'minecraft:loom',
        'minecraft:magenta_concrete',
        'minecraft:magenta_concrete_powder',
        'minecraft:magenta_glazed_terracotta',
        'minecraft:magenta_terracotta',
        'minecraft:magenta_wool',
        'minecraft:magma_block',
        'minecraft:mangrove_log',
        'minecraft:mangrove_planks',
        'minecraft:mangrove_wood',
        'minecraft:melon',
        'minecraft:moss_block',
        'minecraft:mossy_cobblestone',
        'minecraft:mossy_stone_bricks',
        'minecraft:mud',
        'minecraft:mud_bricks',
        'minecraft:mycelium',
        'minecraft:nether_bricks',
        'minecraft:nether_gold_ore',
        'minecraft:nether_quartz_ore',
        'minecraft:nether_wart_block',
        'minecraft:netherite_block',
        'minecraft:netherrack',
        'minecraft:note_block',
        'minecraft:oak_log',
        'minecraft:oak_planks',
        'minecraft:oak_wood',
        'minecraft:observer',
        'minecraft:obsidian',
        'minecraft:ochre_froglight',
        'minecraft:orange_concrete',
        'minecraft:orange_concrete_powder',
        'minecraft:orange_glazed_terracotta',
        'minecraft:orange_terracotta',
        'minecraft:orange_wool',
        'minecraft:oxidized_chiseled_copper',
        'minecraft:oxidized_copper',
        'minecraft:oxidized_copper_bulb',
        'minecraft:oxidized_cut_copper',
        'minecraft:packed_ice',
        'minecraft:packed_mud',
        'minecraft:pale_moss_block',
        'minecraft:pale_oak_log',
        'minecraft:pale_oak_planks',
        'minecraft:pale_oak_wood',
        'minecraft:pearlescent_froglight',
        'minecraft:pink_concrete',
        'minecraft:pink_concrete_powder',
        'minecraft:pink_glazed_terracotta',
        'minecraft:pink_terracotta',
        'minecraft:pink_wool',
        'minecraft:podzol',
        'minecraft:polished_andesite',
        'minecraft:polished_basalt',
        'minecraft:polished_blackstone',
        'minecraft:polished_blackstone_bricks',
        'minecraft:polished_deepslate',
        'minecraft:polished_diorite',
        'minecraft:polished_granite',
        'minecraft:polished_tuff',
        'minecraft:powder_snow',
        'minecraft:prismarine',
        'minecraft:prismarine_bricks',
        'minecraft:pumpkin',
        'minecraft:purple_concrete',
        'minecraft:purple_concrete_powder',
        'minecraft:purple_glazed_terracotta',
        'minecraft:purple_terracotta',
        'minecraft:purple_wool',
        'minecraft:purpur_block',
        'minecraft:purpur_pillar',
        'minecraft:quartz_block',
        'minecraft:quartz_bricks',
        'minecraft:quartz_pillar',
        'minecraft:raw_copper_block',
        'minecraft:raw_gold_block',
        'minecraft:raw_iron_block',
        'minecraft:red_concrete',
        'minecraft:red_concrete_powder',
        'minecraft:red_glazed_terracotta',
        'minecraft:red_mushroom_block',
        'minecraft:red_nether_bricks',
        'minecraft:red_sand',
        'minecraft:red_sandstone',
        'minecraft:red_terracotta',
        'minecraft:red_wool',
        'minecraft:redstone_block',
        'minecraft:redstone_lamp',
        'minecraft:redstone_ore',
        'minecraft:repeating_command_block',
        'minecraft:resin_block',
        'minecraft:resin_bricks',
        'minecraft:respawn_anchor',
        'minecraft:rooted_dirt',
        'minecraft:sand',
        'minecraft:sandstone',
        'minecraft:sculk',
        'minecraft:sculk_catalyst',
        'minecraft:sea_lantern',
        'minecraft:shroomlight',
        'minecraft:smithing_table',
        'minecraft:smoker',
        'minecraft:smooth_basalt',
        'minecraft:smooth_quartz',
        'minecraft:smooth_red_sandstone',
        'minecraft:smooth_sandstone',
        'minecraft:smooth_stone',
        'minecraft:snow_block',
        'minecraft:soul_sand',
        'minecraft:soul_soil',
        'minecraft:sponge',
        'minecraft:spruce_log',
        'minecraft:spruce_planks',
        'minecraft:spruce_wood',
        'minecraft:stone',
        'minecraft:stone_bricks',
        'minecraft:stripped_acacia_log',
        'minecraft:stripped_acacia_wood',
        'minecraft:stripped_bamboo_block',
        'minecraft:stripped_birch_log',
        'minecraft:stripped_birch_wood',
        'minecraft:stripped_cherry_log',
        'minecraft:stripped_cherry_wood',
        'minecraft:stripped_crimson_hyphae',
        'minecraft:stripped_crimson_stem',
        'minecraft:stripped_dark_oak_log',
        'minecraft:stripped_dark_oak_wood',
        'minecraft:stripped_jungle_log',
        'minecraft:stripped_jungle_wood',
        'minecraft:stripped_mangrove_log',
        'minecraft:stripped_mangrove_wood',
        'minecraft:stripped_oak_log',
        'minecraft:stripped_oak_wood',
        'minecraft:stripped_pale_oak_log',
        'minecraft:stripped_pale_oak_wood',
        'minecraft:stripped_spruce_log',
        'minecraft:stripped_spruce_wood',
        'minecraft:stripped_warped_hyphae',
        'minecraft:stripped_warped_stem',
        'minecraft:structure_block',
        'minecraft:suspicious_gravel',
        'minecraft:suspicious_sand',
        'minecraft:target',
        'minecraft:terracotta',
        'minecraft:test_block',
        'minecraft:test_instance_block',
        'minecraft:tnt',
        'minecraft:tube_coral_block',
        'minecraft:tuff',
        'minecraft:tuff_bricks',
        'minecraft:verdant_froglight',
        'minecraft:warped_hyphae',
        'minecraft:warped_nylium',
        'minecraft:warped_planks',
        'minecraft:warped_stem',
        'minecraft:warped_wart_block',
        'minecraft:waxed_chiseled_copper',
        'minecraft:waxed_copper_block',
        'minecraft:waxed_copper_bulb',
        'minecraft:waxed_cut_copper',
        'minecraft:waxed_exposed_chiseled_copper',
        'minecraft:waxed_exposed_copper',
        'minecraft:waxed_exposed_copper_bulb',
        'minecraft:waxed_exposed_cut_copper',
        'minecraft:waxed_oxidized_chiseled_copper',
        'minecraft:waxed_oxidized_copper',
        'minecraft:waxed_oxidized_copper_bulb',
        'minecraft:waxed_oxidized_cut_copper',
        'minecraft:waxed_weathered_chiseled_copper',
        'minecraft:waxed_weathered_copper',
        'minecraft:waxed_weathered_copper_bulb',
        'minecraft:waxed_weathered_cut_copper',
        'minecraft:weathered_chiseled_copper',
        'minecraft:weathered_copper',
        'minecraft:weathered_copper_bulb',
        'minecraft:weathered_cut_copper',
        'minecraft:wet_sponge',
        'minecraft:white_concrete',
        'minecraft:white_concrete_powder',
        'minecraft:white_glazed_terracotta',
        'minecraft:white_terracotta',
        'minecraft:white_wool',
        'minecraft:yellow_concrete',
        'minecraft:yellow_concrete_powder',
        'minecraft:yellow_glazed_terracotta',
        'minecraft:yellow_terracotta',
        'minecraft:yellow_wool',
    ]);
    const TRANSLUCENT_BLOCKS = new Set([
        'minecraft:black_stained_glass',
        'minecraft:black_stained_glass_pane',
        'minecraft:blue_stained_glass',
        'minecraft:blue_stained_glass_pane',
        'minecraft:bubble_column',
        'minecraft:brown_stained_glass',
        'minecraft:brown_stained_glass_pane',
        'minecraft:cyan_stained_glass',
        'minecraft:cyan_stained_glass_pane',
        'minecraft:frosted_ice',
        'minecraft:gray_stained_glass',
        'minecraft:gray_stained_glass_pane',
        'minecraft:green_stained_glass',
        'minecraft:green_stained_glass_pane',
        'minecraft:honey_block',
        'minecraft:ice',
        'minecraft:kelp',
        'minecraft:kelp_plant',
        'minecraft:light_blue_stained_glass',
        'minecraft:light_blue_stained_glass_pane',
        'minecraft:light_gray_stained_glass',
        'minecraft:light_gray_stained_glass_pane',
        'minecraft:lime_stained_glass',
        'minecraft:lime_stained_glass_pane',
        'minecraft:magenta_stained_glass',
        'minecraft:magenta_stained_glass_pane',
        'minecraft:orange_stained_glass',
        'minecraft:orange_stained_glass_pane',
        'minecraft:pink_stained_glass',
        'minecraft:pink_stained_glass_pane',
        'minecraft:purple_stained_glass',
        'minecraft:purple_stained_glass_pane',
        'minecraft:red_stained_glass',
        'minecraft:red_stained_glass_pane',
        'minecraft:seagrass',
        'minecraft:slime_block',
        'minecraft:tall_seagrass',
        'minecraft:water',
        'minecraft:white_stained_glass',
        'minecraft:white_stained_glass_pane',
        'minecraft:yellow_stained_glass',
        'minecraft:yellow_stained_glass_pane',
    ]);
    const NON_SELF_CULLING = new Set([
        'minecraft:acacia_leaves',
        'minecraft:azalea_leaves',
        'minecraft:birch_leaves',
        'minecraft:cherry_leaves',
        'minecraft:dark_oak_leaves',
        'minecraft:flowering_azalea_leaves',
        'minecraft:jungle_leaves',
        'minecraft:mangrove_leaves',
        'minecraft:oak_leaves',
        'minecraft:pale_oak_leaves',
        'minecraft:spruce_leaves',
    ]);

    class ResourceManager {
        constructor(blocks, assets, textureAtlas) {
            this.blocks = new Map(Object.entries(blocks)
                .map(([k, v]) => [
                Identifier.create(k).toString(),
                { properties: v[0], default: v[1] },
            ]));
            this.blockDefinitions = {};
            this.blockModels = {};
            this.textureAtlas = TextureAtlas.empty();
            this.loadBlockDefinitions(assets.blockstates);
            this.loadBlockModels(assets.models);
            this.loadBlockAtlas(textureAtlas, assets.textures);
        }
        getBlockDefinition(id) {
            return this.blockDefinitions[id.toString()];
        }
        getBlockModel(id) {
            return this.blockModels[id.toString()];
        }
        getTextureUV(id) {
            return this.textureAtlas.getTextureUV(id);
        }
        getTextureAtlas() {
            return this.textureAtlas.getTextureAtlas();
        }
        getPixelSize() {
            return this.textureAtlas.getPixelSize();
        }
        getBlockFlags(id) {
            const str = id.toString();
            return {
                opaque: OPAQUE_BLOCKS.has(str),
                semi_transparent: TRANSLUCENT_BLOCKS.has(str),
                self_culling: !NON_SELF_CULLING.has(str),
            };
        }
        getBlockProperties(id) {
            var _a, _b;
            return (_b = (_a = this.blocks[id.toString()]) === null || _a === void 0 ? void 0 : _a.properties) !== null && _b !== void 0 ? _b : null;
        }
        getDefaultBlockProperties(id) {
            var _a, _b;
            return (_b = (_a = this.blocks.get(id.toString())) === null || _a === void 0 ? void 0 : _a.default) !== null && _b !== void 0 ? _b : null;
        }
        loadBlockDefinitions(definitions) {
            Object.keys(definitions).forEach(id => {
                this.blockDefinitions[Identifier.create(id).toString()] = BlockDefinition.fromJson(definitions[id]);
            });
        }
        loadBlockModels(models) {
            Object.keys(models).forEach(id => {
                this.blockModels[Identifier.create(id).toString()] = BlockModel.fromJson(models[id]);
            });
            const builtinEntityId = Identifier.parse('minecraft:builtin/entity').toString();
            if (!this.blockModels[builtinEntityId]) {
                this.blockModels[builtinEntityId] = new BlockModel(undefined, undefined, undefined, undefined, undefined);
            }
            Object.values(this.blockModels).forEach(m => m.flatten(this));
        }
        loadBlockAtlas(image, textures) {
            const atlasCanvas = document.createElement('canvas');
            const w = upperPowerOfTwo(image.width);
            const h = upperPowerOfTwo(image.height);
            atlasCanvas.width = w;
            atlasCanvas.height = h;
            const atlasCtx = atlasCanvas.getContext('2d');
            atlasCtx.drawImage(image, 0, 0);
            const atlasData = atlasCtx.getImageData(0, 0, w, h);
            const idMap = {};
            Object.keys(textures).forEach(id => {
                const [u, v, du, dv] = textures[id];
                const dv2 = (du !== dv && id.startsWith('block/')) ? du : dv;
                idMap[Identifier.create(id).toString()] = [u / w, v / h, (u + du) / w, (v + dv2) / h];
            });
            this.textureAtlas = new TextureAtlas(atlasData, idMap);
        }
    }

    // https://github.com/EngineHub/WorldEdit/blob/master/worldedit-core/src/main/resources/com/sk89q/worldedit/world/registry/legacy.json
    const alphaMaterials = {
        '0:0': 'air',
        '1:0': 'stone',
        '1:1': 'granite',
        '1:2': 'polished_granite',
        '1:3': 'diorite',
        '1:4': 'polished_diorite',
        '1:5': 'andesite',
        '1:6': 'polished_andesite',
        '2:0': 'grass_block[snowy=false]',
        '3:0': 'dirt',
        '3:1': 'coarse_dirt',
        '3:2': 'podzol[snowy=false]',
        '4:0': 'cobblestone',
        '5:0': 'oak_planks',
        '5:1': 'spruce_planks',
        '5:2': 'birch_planks',
        '5:3': 'jungle_planks',
        '5:4': 'acacia_planks',
        '5:5': 'dark_oak_planks',
        '6:0': 'oak_sapling[stage=0]',
        '6:1': 'spruce_sapling[stage=0]',
        '6:2': 'birch_sapling[stage=0]',
        '6:3': 'jungle_sapling[stage=0]',
        '6:4': 'acacia_sapling[stage=0]',
        '6:5': 'dark_oak_sapling[stage=0]',
        '6:8': 'oak_sapling[stage=1]',
        '6:9': 'spruce_sapling[stage=1]',
        '6:10': 'birch_sapling[stage=1]',
        '6:11': 'jungle_sapling[stage=1]',
        '6:12': 'acacia_sapling[stage=1]',
        '6:13': 'dark_oak_sapling[stage=1]',
        '7:0': 'bedrock',
        '8:0': 'water[level=0]',
        '8:1': 'water[level=1]',
        '8:2': 'water[level=2]',
        '8:3': 'water[level=3]',
        '8:4': 'water[level=4]',
        '8:5': 'water[level=5]',
        '8:6': 'water[level=6]',
        '8:7': 'water[level=7]',
        '8:8': 'water[level=8]',
        '8:9': 'water[level=9]',
        '8:10': 'water[level=10]',
        '8:11': 'water[level=11]',
        '8:12': 'water[level=12]',
        '8:13': 'water[level=13]',
        '8:14': 'water[level=14]',
        '8:15': 'water[level=15]',
        '9:0': 'water[level=0]',
        '9:1': 'water[level=1]',
        '9:2': 'water[level=2]',
        '9:3': 'water[level=3]',
        '9:4': 'water[level=4]',
        '9:5': 'water[level=5]',
        '9:6': 'water[level=6]',
        '9:7': 'water[level=7]',
        '9:8': 'water[level=8]',
        '9:9': 'water[level=9]',
        '9:10': 'water[level=10]',
        '9:11': 'water[level=11]',
        '9:12': 'water[level=12]',
        '9:13': 'water[level=13]',
        '9:14': 'water[level=14]',
        '9:15': 'water[level=15]',
        '10:0': 'lava[level=0]',
        '10:1': 'lava[level=1]',
        '10:2': 'lava[level=2]',
        '10:3': 'lava[level=3]',
        '10:4': 'lava[level=4]',
        '10:5': 'lava[level=5]',
        '10:6': 'lava[level=6]',
        '10:7': 'lava[level=7]',
        '10:8': 'lava[level=8]',
        '10:9': 'lava[level=9]',
        '10:10': 'lava[level=10]',
        '10:11': 'lava[level=11]',
        '10:12': 'lava[level=12]',
        '10:13': 'lava[level=13]',
        '10:14': 'lava[level=14]',
        '10:15': 'lava[level=15]',
        '11:0': 'lava[level=0]',
        '11:1': 'lava[level=1]',
        '11:2': 'lava[level=2]',
        '11:3': 'lava[level=3]',
        '11:4': 'lava[level=4]',
        '11:5': 'lava[level=5]',
        '11:6': 'lava[level=6]',
        '11:7': 'lava[level=7]',
        '11:8': 'lava[level=8]',
        '11:9': 'lava[level=9]',
        '11:10': 'lava[level=10]',
        '11:11': 'lava[level=11]',
        '11:12': 'lava[level=12]',
        '11:13': 'lava[level=13]',
        '11:14': 'lava[level=14]',
        '11:15': 'lava[level=15]',
        '12:0': 'sand',
        '12:1': 'red_sand',
        '13:0': 'gravel',
        '14:0': 'gold_ore',
        '15:0': 'iron_ore',
        '16:0': 'coal_ore',
        '17:0': 'oak_log[axis=y]',
        '17:1': 'spruce_log[axis=y]',
        '17:2': 'birch_log[axis=y]',
        '17:3': 'jungle_log[axis=y]',
        '17:4': 'oak_log[axis=x]',
        '17:5': 'spruce_log[axis=x]',
        '17:6': 'birch_log[axis=x]',
        '17:7': 'jungle_log[axis=x]',
        '17:8': 'oak_log[axis=z]',
        '17:9': 'spruce_log[axis=z]',
        '17:10': 'birch_log[axis=z]',
        '17:11': 'jungle_log[axis=z]',
        '17:12': 'oak_wood',
        '17:13': 'spruce_wood',
        '17:14': 'birch_wood',
        '17:15': 'jungle_wood',
        '18:0': 'oak_leaves[persistent=false,distance=1]',
        '18:1': 'spruce_leaves[persistent=false,distance=1]',
        '18:2': 'birch_leaves[persistent=false,distance=1]',
        '18:3': 'jungle_leaves[persistent=false,distance=1]',
        '18:4': 'oak_leaves[persistent=true,distance=1]',
        '18:5': 'spruce_leaves[persistent=true,distance=1]',
        '18:6': 'birch_leaves[persistent=true,distance=1]',
        '18:7': 'jungle_leaves[persistent=true,distance=1]',
        '18:8': 'oak_leaves[persistent=false,distance=1]',
        '18:9': 'spruce_leaves[persistent=false,distance=1]',
        '18:10': 'birch_leaves[persistent=false,distance=1]',
        '18:11': 'jungle_leaves[persistent=false,distance=1]',
        '18:12': 'oak_leaves[persistent=true,distance=1]',
        '18:13': 'spruce_leaves[persistent=true,distance=1]',
        '18:14': 'birch_leaves[persistent=true,distance=1]',
        '18:15': 'jungle_leaves[persistent=true,distance=1]',
        '19:0': 'sponge',
        '19:1': 'wet_sponge',
        '20:0': 'glass',
        '21:0': 'lapis_ore',
        '22:0': 'lapis_block',
        '23:0': 'dispenser[triggered=false,facing=down]',
        '23:1': 'dispenser[triggered=false,facing=up]',
        '23:2': 'dispenser[triggered=false,facing=north]',
        '23:3': 'dispenser[triggered=false,facing=south]',
        '23:4': 'dispenser[triggered=false,facing=west]',
        '23:5': 'dispenser[triggered=false,facing=east]',
        '23:8': 'dispenser[triggered=true,facing=down]',
        '23:9': 'dispenser[triggered=true,facing=up]',
        '23:10': 'dispenser[triggered=true,facing=north]',
        '23:11': 'dispenser[triggered=true,facing=south]',
        '23:12': 'dispenser[triggered=true,facing=west]',
        '23:13': 'dispenser[triggered=true,facing=east]',
        '24:0': 'sandstone',
        '24:1': 'chiseled_sandstone',
        '24:2': 'cut_sandstone',
        '25:0': 'note_block',
        '26:0': 'red_bed[part=foot,facing=south,occupied=false]',
        '26:1': 'red_bed[part=foot,facing=west,occupied=false]',
        '26:2': 'red_bed[part=foot,facing=north,occupied=false]',
        '26:3': 'red_bed[part=foot,facing=east,occupied=false]',
        '26:4': 'red_bed[part=foot,facing=south,occupied=true]',
        '26:5': 'red_bed[part=foot,facing=west,occupied=true]',
        '26:6': 'red_bed[part=foot,facing=north,occupied=true]',
        '26:7': 'red_bed[part=foot,facing=east,occupied=true]',
        '26:8': 'red_bed[part=head,facing=south,occupied=false]',
        '26:9': 'red_bed[part=head,facing=west,occupied=false]',
        '26:10': 'red_bed[part=head,facing=north,occupied=false]',
        '26:11': 'red_bed[part=head,facing=east,occupied=false]',
        '26:12': 'red_bed[part=head,facing=south,occupied=true]',
        '26:13': 'red_bed[part=head,facing=west,occupied=true]',
        '26:14': 'red_bed[part=head,facing=north,occupied=true]',
        '26:15': 'red_bed[part=head,facing=east,occupied=true]',
        '27:0': 'powered_rail[shape=north_south,powered=false]',
        '27:1': 'powered_rail[shape=east_west,powered=false]',
        '27:2': 'powered_rail[shape=ascending_east,powered=false]',
        '27:3': 'powered_rail[shape=ascending_west,powered=false]',
        '27:4': 'powered_rail[shape=ascending_north,powered=false]',
        '27:5': 'powered_rail[shape=ascending_south,powered=false]',
        '27:8': 'powered_rail[shape=north_south,powered=true]',
        '27:9': 'powered_rail[shape=east_west,powered=true]',
        '27:10': 'powered_rail[shape=ascending_east,powered=true]',
        '27:11': 'powered_rail[shape=ascending_west,powered=true]',
        '27:12': 'powered_rail[shape=ascending_north,powered=true]',
        '27:13': 'powered_rail[shape=ascending_south,powered=true]',
        '28:0': 'detector_rail[shape=north_south,powered=false]',
        '28:1': 'detector_rail[shape=east_west,powered=false]',
        '28:2': 'detector_rail[shape=ascending_east,powered=false]',
        '28:3': 'detector_rail[shape=ascending_west,powered=false]',
        '28:4': 'detector_rail[shape=ascending_north,powered=false]',
        '28:5': 'detector_rail[shape=ascending_south,powered=false]',
        '28:8': 'detector_rail[shape=north_south,powered=true]',
        '28:9': 'detector_rail[shape=east_west,powered=true]',
        '28:10': 'detector_rail[shape=ascending_east,powered=true]',
        '28:11': 'detector_rail[shape=ascending_west,powered=true]',
        '28:12': 'detector_rail[shape=ascending_north,powered=true]',
        '28:13': 'detector_rail[shape=ascending_south,powered=true]',
        '29:0': 'sticky_piston[facing=down,extended=false]',
        '29:1': 'sticky_piston[facing=up,extended=false]',
        '29:2': 'sticky_piston[facing=north,extended=false]',
        '29:3': 'sticky_piston[facing=south,extended=false]',
        '29:4': 'sticky_piston[facing=west,extended=false]',
        '29:5': 'sticky_piston[facing=east,extended=false]',
        '29:8': 'sticky_piston[facing=down,extended=true]',
        '29:9': 'sticky_piston[facing=up,extended=true]',
        '29:10': 'sticky_piston[facing=north,extended=true]',
        '29:11': 'sticky_piston[facing=south,extended=true]',
        '29:12': 'sticky_piston[facing=west,extended=true]',
        '29:13': 'sticky_piston[facing=east,extended=true]',
        '30:0': 'cobweb',
        '31:0': 'dead_bush',
        '31:1': 'grass',
        '31:2': 'fern',
        '32:0': 'dead_bush',
        '33:0': 'piston[facing=down,extended=false]',
        '33:1': 'piston[facing=up,extended=false]',
        '33:2': 'piston[facing=north,extended=false]',
        '33:3': 'piston[facing=south,extended=false]',
        '33:4': 'piston[facing=west,extended=false]',
        '33:5': 'piston[facing=east,extended=false]',
        '33:8': 'piston[facing=down,extended=true]',
        '33:9': 'piston[facing=up,extended=true]',
        '33:10': 'piston[facing=north,extended=true]',
        '33:11': 'piston[facing=south,extended=true]',
        '33:12': 'piston[facing=west,extended=true]',
        '33:13': 'piston[facing=east,extended=true]',
        '34:0': 'piston_head[short=false,facing=down,type=normal]',
        '34:1': 'piston_head[short=false,facing=up,type=normal]',
        '34:2': 'piston_head[short=false,facing=north,type=normal]',
        '34:3': 'piston_head[short=false,facing=south,type=normal]',
        '34:4': 'piston_head[short=false,facing=west,type=normal]',
        '34:5': 'piston_head[short=false,facing=east,type=normal]',
        '34:8': 'piston_head[short=false,facing=down,type=sticky]',
        '34:9': 'piston_head[short=false,facing=up,type=sticky]',
        '34:10': 'piston_head[short=false,facing=north,type=sticky]',
        '34:11': 'piston_head[short=false,facing=south,type=sticky]',
        '34:12': 'piston_head[short=false,facing=west,type=sticky]',
        '34:13': 'piston_head[short=false,facing=east,type=sticky]',
        '35:0': 'white_wool',
        '35:1': 'orange_wool',
        '35:2': 'magenta_wool',
        '35:3': 'light_blue_wool',
        '35:4': 'yellow_wool',
        '35:5': 'lime_wool',
        '35:6': 'pink_wool',
        '35:7': 'gray_wool',
        '35:8': 'light_gray_wool',
        '35:9': 'cyan_wool',
        '35:10': 'purple_wool',
        '35:11': 'blue_wool',
        '35:12': 'brown_wool',
        '35:13': 'green_wool',
        '35:14': 'red_wool',
        '35:15': 'black_wool',
        '36:0': 'moving_piston[facing=down,type=normal]',
        '36:1': 'moving_piston[facing=up,type=normal]',
        '36:2': 'moving_piston[facing=north,type=normal]',
        '36:3': 'moving_piston[facing=south,type=normal]',
        '36:4': 'moving_piston[facing=west,type=normal]',
        '36:5': 'moving_piston[facing=east,type=normal]',
        '36:8': 'moving_piston[facing=down,type=sticky]',
        '36:9': 'moving_piston[facing=up,type=sticky]',
        '36:10': 'moving_piston[facing=north,type=sticky]',
        '36:11': 'moving_piston[facing=south,type=sticky]',
        '36:12': 'moving_piston[facing=west,type=sticky]',
        '36:13': 'moving_piston[facing=east,type=sticky]',
        '37:0': 'dandelion',
        '38:0': 'poppy',
        '38:1': 'blue_orchid',
        '38:2': 'allium',
        '38:3': 'azure_bluet',
        '38:4': 'red_tulip',
        '38:5': 'orange_tulip',
        '38:6': 'white_tulip',
        '38:7': 'pink_tulip',
        '38:8': 'oxeye_daisy',
        '39:0': 'brown_mushroom',
        '40:0': 'red_mushroom',
        '41:0': 'gold_block',
        '42:0': 'iron_block',
        '43:0': 'stone_slab[type=double]',
        '43:1': 'sandstone_slab[type=double]',
        '43:2': 'petrified_oak_slab[type=double]',
        '43:3': 'cobblestone_slab[type=double]',
        '43:4': 'brick_slab[type=double]',
        '43:5': 'stone_brick_slab[type=double]',
        '43:6': 'nether_brick_slab[type=double]',
        '43:7': 'quartz_slab[type=double]',
        '43:8': 'smooth_stone',
        '43:9': 'smooth_sandstone',
        '43:10': 'petrified_oak_slab[type=double]',
        '43:11': 'cobblestone_slab[type=double]',
        '43:12': 'brick_slab[type=double]',
        '43:13': 'stone_brick_slab[type=double]',
        '43:14': 'nether_brick_slab[type=double]',
        '43:15': 'smooth_quartz',
        '44:0': 'stone_slab[type=bottom]',
        '44:1': 'sandstone_slab[type=bottom]',
        '44:2': 'petrified_oak_slab[type=bottom]',
        '44:3': 'cobblestone_slab[type=bottom]',
        '44:4': 'brick_slab[type=bottom]',
        '44:5': 'stone_brick_slab[type=bottom]',
        '44:6': 'nether_brick_slab[type=bottom]',
        '44:7': 'quartz_slab[type=bottom]',
        '44:8': 'stone_slab[type=top]',
        '44:9': 'sandstone_slab[type=top]',
        '44:10': 'petrified_oak_slab[type=top]',
        '44:11': 'cobblestone_slab[type=top]',
        '44:12': 'brick_slab[type=top]',
        '44:13': 'stone_brick_slab[type=top]',
        '44:14': 'nether_brick_slab[type=top]',
        '44:15': 'quartz_slab[type=top]',
        '45:0': 'bricks',
        '46:0': 'tnt[unstable=false]',
        '46:1': 'tnt[unstable=true]',
        '47:0': 'bookshelf',
        '48:0': 'mossy_cobblestone',
        '49:0': 'obsidian',
        '50:1': 'wall_torch[facing=east]',
        '50:2': 'wall_torch[facing=west]',
        '50:3': 'wall_torch[facing=south]',
        '50:4': 'wall_torch[facing=north]',
        '50:5': 'torch',
        '51:0': 'fire[east=false,south=false,north=false,west=false,up=false,age=0]',
        '51:1': 'fire[east=false,south=false,north=false,west=false,up=false,age=1]',
        '51:2': 'fire[east=false,south=false,north=false,west=false,up=false,age=2]',
        '51:3': 'fire[east=false,south=false,north=false,west=false,up=false,age=3]',
        '51:4': 'fire[east=false,south=false,north=false,west=false,up=false,age=4]',
        '51:5': 'fire[east=false,south=false,north=false,west=false,up=false,age=5]',
        '51:6': 'fire[east=false,south=false,north=false,west=false,up=false,age=6]',
        '51:7': 'fire[east=false,south=false,north=false,west=false,up=false,age=7]',
        '51:8': 'fire[east=false,south=false,north=false,west=false,up=false,age=8]',
        '51:9': 'fire[east=false,south=false,north=false,west=false,up=false,age=9]',
        '51:10': 'fire[east=false,south=false,north=false,west=false,up=false,age=10]',
        '51:11': 'fire[east=false,south=false,north=false,west=false,up=false,age=11]',
        '51:12': 'fire[east=false,south=false,north=false,west=false,up=false,age=12]',
        '51:13': 'fire[east=false,south=false,north=false,west=false,up=false,age=13]',
        '51:14': 'fire[east=false,south=false,north=false,west=false,up=false,age=14]',
        '51:15': 'fire[east=false,south=false,north=false,west=false,up=false,age=15]',
        '52:0': 'spawner',
        '53:0': 'oak_stairs[half=bottom,shape=straight,facing=east]',
        '53:1': 'oak_stairs[half=bottom,shape=straight,facing=west]',
        '53:2': 'oak_stairs[half=bottom,shape=straight,facing=south]',
        '53:3': 'oak_stairs[half=bottom,shape=straight,facing=north]',
        '53:4': 'oak_stairs[half=top,shape=straight,facing=east]',
        '53:5': 'oak_stairs[half=top,shape=straight,facing=west]',
        '53:6': 'oak_stairs[half=top,shape=straight,facing=south]',
        '53:7': 'oak_stairs[half=top,shape=straight,facing=north]',
        '54:2': 'chest[facing=north,type=single]',
        '54:3': 'chest[facing=south,type=single]',
        '54:4': 'chest[facing=west,type=single]',
        '54:5': 'chest[facing=east,type=single]',
        '55:0': 'redstone_wire[east=none,south=none,north=none,west=none,power=0]',
        '55:1': 'redstone_wire[east=none,south=none,north=none,west=none,power=1]',
        '55:2': 'redstone_wire[east=none,south=none,north=none,west=none,power=2]',
        '55:3': 'redstone_wire[east=none,south=none,north=none,west=none,power=3]',
        '55:4': 'redstone_wire[east=none,south=none,north=none,west=none,power=4]',
        '55:5': 'redstone_wire[east=none,south=none,north=none,west=none,power=5]',
        '55:6': 'redstone_wire[east=none,south=none,north=none,west=none,power=6]',
        '55:7': 'redstone_wire[east=none,south=none,north=none,west=none,power=7]',
        '55:8': 'redstone_wire[east=none,south=none,north=none,west=none,power=8]',
        '55:9': 'redstone_wire[east=none,south=none,north=none,west=none,power=9]',
        '55:10': 'redstone_wire[east=none,south=none,north=none,west=none,power=10]',
        '55:11': 'redstone_wire[east=none,south=none,north=none,west=none,power=11]',
        '55:12': 'redstone_wire[east=none,south=none,north=none,west=none,power=12]',
        '55:13': 'redstone_wire[east=none,south=none,north=none,west=none,power=13]',
        '55:14': 'redstone_wire[east=none,south=none,north=none,west=none,power=14]',
        '55:15': 'redstone_wire[east=none,south=none,north=none,west=none,power=15]',
        '56:0': 'diamond_ore',
        '57:0': 'diamond_block',
        '58:0': 'crafting_table',
        '59:0': 'wheat[age=0]',
        '59:1': 'wheat[age=1]',
        '59:2': 'wheat[age=2]',
        '59:3': 'wheat[age=3]',
        '59:4': 'wheat[age=4]',
        '59:5': 'wheat[age=5]',
        '59:6': 'wheat[age=6]',
        '59:7': 'wheat[age=7]',
        '60:0': 'farmland[moisture=0]',
        '60:1': 'farmland[moisture=1]',
        '60:2': 'farmland[moisture=2]',
        '60:3': 'farmland[moisture=3]',
        '60:4': 'farmland[moisture=4]',
        '60:5': 'farmland[moisture=5]',
        '60:6': 'farmland[moisture=6]',
        '60:7': 'farmland[moisture=7]',
        '61:2': 'furnace[facing=north,lit=false]',
        '61:3': 'furnace[facing=south,lit=false]',
        '61:4': 'furnace[facing=west,lit=false]',
        '61:5': 'furnace[facing=east,lit=false]',
        '62:2': 'furnace[facing=north,lit=true]',
        '62:3': 'furnace[facing=south,lit=true]',
        '62:4': 'furnace[facing=west,lit=true]',
        '62:5': 'furnace[facing=east,lit=true]',
        '63:0': 'sign[rotation=0]',
        '63:1': 'sign[rotation=1]',
        '63:2': 'sign[rotation=2]',
        '63:3': 'sign[rotation=3]',
        '63:4': 'sign[rotation=4]',
        '63:5': 'sign[rotation=5]',
        '63:6': 'sign[rotation=6]',
        '63:7': 'sign[rotation=7]',
        '63:8': 'sign[rotation=8]',
        '63:9': 'sign[rotation=9]',
        '63:10': 'sign[rotation=10]',
        '63:11': 'sign[rotation=11]',
        '63:12': 'sign[rotation=12]',
        '63:13': 'sign[rotation=13]',
        '63:14': 'sign[rotation=14]',
        '63:15': 'sign[rotation=15]',
        '64:0': 'oak_door[hinge=right,half=lower,powered=false,facing=east,open=false]',
        '64:1': 'oak_door[hinge=right,half=lower,powered=false,facing=south,open=false]',
        '64:2': 'oak_door[hinge=right,half=lower,powered=false,facing=west,open=false]',
        '64:3': 'oak_door[hinge=right,half=lower,powered=false,facing=north,open=false]',
        '64:4': 'oak_door[hinge=right,half=lower,powered=false,facing=east,open=true]',
        '64:5': 'oak_door[hinge=right,half=lower,powered=false,facing=south,open=true]',
        '64:6': 'oak_door[hinge=right,half=lower,powered=false,facing=west,open=true]',
        '64:7': 'oak_door[hinge=right,half=lower,powered=false,facing=north,open=true]',
        '64:8': 'oak_door[hinge=left,half=upper,powered=false,facing=east,open=false]',
        '64:9': 'oak_door[hinge=right,half=upper,powered=false,facing=east,open=false]',
        '64:10': 'oak_door[hinge=left,half=upper,powered=true,facing=east,open=false]',
        '64:11': 'oak_door[hinge=right,half=upper,powered=true,facing=east,open=false]',
        '65:2': 'ladder[facing=north]',
        '65:3': 'ladder[facing=south]',
        '65:4': 'ladder[facing=west]',
        '65:5': 'ladder[facing=east]',
        '66:0': 'rail[shape=north_south]',
        '66:1': 'rail[shape=east_west]',
        '66:2': 'rail[shape=ascending_east]',
        '66:3': 'rail[shape=ascending_west]',
        '66:4': 'rail[shape=ascending_north]',
        '66:5': 'rail[shape=ascending_south]',
        '66:6': 'rail[shape=south_east]',
        '66:7': 'rail[shape=south_west]',
        '66:8': 'rail[shape=north_west]',
        '66:9': 'rail[shape=north_east]',
        '67:0': 'cobblestone_stairs[half=bottom,shape=straight,facing=east]',
        '67:1': 'cobblestone_stairs[half=bottom,shape=straight,facing=west]',
        '67:2': 'cobblestone_stairs[half=bottom,shape=straight,facing=south]',
        '67:3': 'cobblestone_stairs[half=bottom,shape=straight,facing=north]',
        '67:4': 'cobblestone_stairs[half=top,shape=straight,facing=east]',
        '67:5': 'cobblestone_stairs[half=top,shape=straight,facing=west]',
        '67:6': 'cobblestone_stairs[half=top,shape=straight,facing=south]',
        '67:7': 'cobblestone_stairs[half=top,shape=straight,facing=north]',
        '68:2': 'wall_sign[facing=north]',
        '68:3': 'wall_sign[facing=south]',
        '68:4': 'wall_sign[facing=west]',
        '68:5': 'wall_sign[facing=east]',
        '69:0': 'lever[powered=false,facing=north,face=ceiling]',
        '69:1': 'lever[powered=false,facing=east,face=wall]',
        '69:2': 'lever[powered=false,facing=west,face=wall]',
        '69:3': 'lever[powered=false,facing=south,face=wall]',
        '69:4': 'lever[powered=false,facing=north,face=wall]',
        '69:5': 'lever[powered=false,facing=east,face=floor]',
        '69:6': 'lever[powered=false,facing=north,face=floor]',
        '69:7': 'lever[powered=false,facing=east,face=ceiling]',
        '69:8': 'lever[powered=true,facing=north,face=ceiling]',
        '69:9': 'lever[powered=true,facing=east,face=wall]',
        '69:10': 'lever[powered=true,facing=west,face=wall]',
        '69:11': 'lever[powered=true,facing=south,face=wall]',
        '69:12': 'lever[powered=true,facing=north,face=wall]',
        '69:13': 'lever[powered=true,facing=east,face=floor]',
        '69:14': 'lever[powered=true,facing=north,face=floor]',
        '69:15': 'lever[powered=true,facing=east,face=ceiling]',
        '70:0': 'stone_pressure_plate[powered=false]',
        '70:1': 'stone_pressure_plate[powered=true]',
        '71:0': 'iron_door[hinge=right,half=lower,powered=false,facing=east,open=false]',
        '71:1': 'iron_door[hinge=right,half=lower,powered=false,facing=south,open=false]',
        '71:2': 'iron_door[hinge=right,half=lower,powered=false,facing=west,open=false]',
        '71:3': 'iron_door[hinge=right,half=lower,powered=false,facing=north,open=false]',
        '71:4': 'iron_door[hinge=right,half=lower,powered=false,facing=east,open=true]',
        '71:5': 'iron_door[hinge=right,half=lower,powered=false,facing=south,open=true]',
        '71:6': 'iron_door[hinge=right,half=lower,powered=false,facing=west,open=true]',
        '71:7': 'iron_door[hinge=right,half=lower,powered=false,facing=north,open=true]',
        '71:8': 'iron_door[hinge=left,half=upper,powered=false,facing=east,open=false]',
        '71:9': 'iron_door[hinge=right,half=upper,powered=false,facing=east,open=false]',
        '71:10': 'iron_door[hinge=left,half=upper,powered=true,facing=east,open=false]',
        '71:11': 'iron_door[hinge=right,half=upper,powered=true,facing=east,open=false]',
        '72:0': 'oak_pressure_plate[powered=false]',
        '72:1': 'oak_pressure_plate[powered=true]',
        '73:0': 'redstone_ore[lit=false]',
        '74:0': 'redstone_ore[lit=true]',
        '75:1': 'redstone_wall_torch[facing=east,lit=false]',
        '75:2': 'redstone_wall_torch[facing=west,lit=false]',
        '75:3': 'redstone_wall_torch[facing=south,lit=false]',
        '75:4': 'redstone_wall_torch[facing=north,lit=false]',
        '75:5': 'redstone_torch[lit=false]',
        '76:1': 'redstone_wall_torch[facing=east,lit=true]',
        '76:2': 'redstone_wall_torch[facing=west,lit=true]',
        '76:3': 'redstone_wall_torch[facing=south,lit=true]',
        '76:4': 'redstone_wall_torch[facing=north,lit=true]',
        '76:5': 'redstone_torch[lit=true]',
        '77:0': 'stone_button[powered=false,facing=east,face=ceiling]',
        '77:1': 'stone_button[powered=false,facing=east,face=wall]',
        '77:2': 'stone_button[powered=false,facing=west,face=wall]',
        '77:3': 'stone_button[powered=false,facing=south,face=wall]',
        '77:4': 'stone_button[powered=false,facing=north,face=wall]',
        '77:5': 'stone_button[powered=false,facing=east,face=floor]',
        '77:8': 'stone_button[powered=true,facing=south,face=ceiling]',
        '77:9': 'stone_button[powered=true,facing=east,face=wall]',
        '77:10': 'stone_button[powered=true,facing=west,face=wall]',
        '77:11': 'stone_button[powered=true,facing=south,face=wall]',
        '77:12': 'stone_button[powered=true,facing=north,face=wall]',
        '77:13': 'stone_button[powered=true,facing=south,face=floor]',
        '78:0': 'snow[layers=1]',
        '78:1': 'snow[layers=2]',
        '78:2': 'snow[layers=3]',
        '78:3': 'snow[layers=4]',
        '78:4': 'snow[layers=5]',
        '78:5': 'snow[layers=6]',
        '78:6': 'snow[layers=7]',
        '78:7': 'snow[layers=8]',
        '79:0': 'ice',
        '80:0': 'snow_block',
        '81:0': 'cactus[age=0]',
        '81:1': 'cactus[age=1]',
        '81:2': 'cactus[age=2]',
        '81:3': 'cactus[age=3]',
        '81:4': 'cactus[age=4]',
        '81:5': 'cactus[age=5]',
        '81:6': 'cactus[age=6]',
        '81:7': 'cactus[age=7]',
        '81:8': 'cactus[age=8]',
        '81:9': 'cactus[age=9]',
        '81:10': 'cactus[age=10]',
        '81:11': 'cactus[age=11]',
        '81:12': 'cactus[age=12]',
        '81:13': 'cactus[age=13]',
        '81:14': 'cactus[age=14]',
        '81:15': 'cactus[age=15]',
        '82:0': 'clay',
        '83:0': 'sugar_cane[age=0]',
        '83:1': 'sugar_cane[age=1]',
        '83:2': 'sugar_cane[age=2]',
        '83:3': 'sugar_cane[age=3]',
        '83:4': 'sugar_cane[age=4]',
        '83:5': 'sugar_cane[age=5]',
        '83:6': 'sugar_cane[age=6]',
        '83:7': 'sugar_cane[age=7]',
        '83:8': 'sugar_cane[age=8]',
        '83:9': 'sugar_cane[age=9]',
        '83:10': 'sugar_cane[age=10]',
        '83:11': 'sugar_cane[age=11]',
        '83:12': 'sugar_cane[age=12]',
        '83:13': 'sugar_cane[age=13]',
        '83:14': 'sugar_cane[age=14]',
        '83:15': 'sugar_cane[age=15]',
        '84:0': 'jukebox[has_record=false]',
        '84:1': 'jukebox[has_record=true]',
        '85:0': 'oak_fence[east=false,south=false,north=false,west=false]',
        '86:0': 'carved_pumpkin[facing=south]',
        '86:1': 'carved_pumpkin[facing=west]',
        '86:2': 'carved_pumpkin[facing=north]',
        '86:3': 'carved_pumpkin[facing=east]',
        '87:0': 'netherrack',
        '88:0': 'soul_sand',
        '89:0': 'glowstone',
        '90:1': 'nether_portal[axis=x]',
        '90:2': 'nether_portal[axis=z]',
        '91:0': 'jack_o_lantern[facing=south]',
        '91:1': 'jack_o_lantern[facing=west]',
        '91:2': 'jack_o_lantern[facing=north]',
        '91:3': 'jack_o_lantern[facing=east]',
        '92:0': 'cake[bites=0]',
        '92:1': 'cake[bites=1]',
        '92:2': 'cake[bites=2]',
        '92:3': 'cake[bites=3]',
        '92:4': 'cake[bites=4]',
        '92:5': 'cake[bites=5]',
        '92:6': 'cake[bites=6]',
        '93:0': 'repeater[delay=1,facing=south,locked=false,powered=false]',
        '93:1': 'repeater[delay=1,facing=west,locked=false,powered=false]',
        '93:2': 'repeater[delay=1,facing=north,locked=false,powered=false]',
        '93:3': 'repeater[delay=1,facing=east,locked=false,powered=false]',
        '93:4': 'repeater[delay=2,facing=south,locked=false,powered=false]',
        '93:5': 'repeater[delay=2,facing=west,locked=false,powered=false]',
        '93:6': 'repeater[delay=2,facing=north,locked=false,powered=false]',
        '93:7': 'repeater[delay=2,facing=east,locked=false,powered=false]',
        '93:8': 'repeater[delay=3,facing=south,locked=false,powered=false]',
        '93:9': 'repeater[delay=3,facing=west,locked=false,powered=false]',
        '93:10': 'repeater[delay=3,facing=north,locked=false,powered=false]',
        '93:11': 'repeater[delay=3,facing=east,locked=false,powered=false]',
        '93:12': 'repeater[delay=4,facing=south,locked=false,powered=false]',
        '93:13': 'repeater[delay=4,facing=west,locked=false,powered=false]',
        '93:14': 'repeater[delay=4,facing=north,locked=false,powered=false]',
        '93:15': 'repeater[delay=4,facing=east,locked=false,powered=false]',
        '94:0': 'repeater[delay=1,facing=south,locked=false,powered=true]',
        '94:1': 'repeater[delay=1,facing=west,locked=false,powered=true]',
        '94:2': 'repeater[delay=1,facing=north,locked=false,powered=true]',
        '94:3': 'repeater[delay=1,facing=east,locked=false,powered=true]',
        '94:4': 'repeater[delay=2,facing=south,locked=false,powered=true]',
        '94:5': 'repeater[delay=2,facing=west,locked=false,powered=true]',
        '94:6': 'repeater[delay=2,facing=north,locked=false,powered=true]',
        '94:7': 'repeater[delay=2,facing=east,locked=false,powered=true]',
        '94:8': 'repeater[delay=3,facing=south,locked=false,powered=true]',
        '94:9': 'repeater[delay=3,facing=west,locked=false,powered=true]',
        '94:10': 'repeater[delay=3,facing=north,locked=false,powered=true]',
        '94:11': 'repeater[delay=3,facing=east,locked=false,powered=true]',
        '94:12': 'repeater[delay=4,facing=south,locked=false,powered=true]',
        '94:13': 'repeater[delay=4,facing=west,locked=false,powered=true]',
        '94:14': 'repeater[delay=4,facing=north,locked=false,powered=true]',
        '94:15': 'repeater[delay=4,facing=east,locked=false,powered=true]',
        '95:0': 'white_stained_glass',
        '95:1': 'orange_stained_glass',
        '95:2': 'magenta_stained_glass',
        '95:3': 'light_blue_stained_glass',
        '95:4': 'yellow_stained_glass',
        '95:5': 'lime_stained_glass',
        '95:6': 'pink_stained_glass',
        '95:7': 'gray_stained_glass',
        '95:8': 'light_gray_stained_glass',
        '95:9': 'cyan_stained_glass',
        '95:10': 'purple_stained_glass',
        '95:11': 'blue_stained_glass',
        '95:12': 'brown_stained_glass',
        '95:13': 'green_stained_glass',
        '95:14': 'red_stained_glass',
        '95:15': 'black_stained_glass',
        '96:0': 'oak_trapdoor[half=bottom,facing=north,open=false,powered=false]',
        '96:1': 'oak_trapdoor[half=bottom,facing=south,open=false,powered=false]',
        '96:2': 'oak_trapdoor[half=bottom,facing=west,open=false,powered=false]',
        '96:3': 'oak_trapdoor[half=bottom,facing=east,open=false,powered=false]',
        '96:4': 'oak_trapdoor[half=bottom,facing=north,open=true,powered=true]',
        '96:5': 'oak_trapdoor[half=bottom,facing=south,open=true,powered=true]',
        '96:6': 'oak_trapdoor[half=bottom,facing=west,open=true,powered=true]',
        '96:7': 'oak_trapdoor[half=bottom,facing=east,open=true,powered=true]',
        '96:8': 'oak_trapdoor[half=top,facing=north,open=false,powered=false]',
        '96:9': 'oak_trapdoor[half=top,facing=south,open=false,powered=false]',
        '96:10': 'oak_trapdoor[half=top,facing=west,open=false,powered=false]',
        '96:11': 'oak_trapdoor[half=top,facing=east,open=false,powered=false]',
        '96:12': 'oak_trapdoor[half=top,facing=north,open=true,powered=true]',
        '96:13': 'oak_trapdoor[half=top,facing=south,open=true,powered=true]',
        '96:14': 'oak_trapdoor[half=top,facing=west,open=true,powered=true]',
        '96:15': 'oak_trapdoor[half=top,facing=east,open=true,powered=true]',
        '97:0': 'infested_stone',
        '97:1': 'infested_cobblestone',
        '97:2': 'infested_stone_bricks',
        '97:3': 'infested_mossy_stone_bricks',
        '97:4': 'infested_cracked_stone_bricks',
        '97:5': 'infested_chiseled_stone_bricks',
        '98:0': 'stone_bricks',
        '98:1': 'mossy_stone_bricks',
        '98:2': 'cracked_stone_bricks',
        '98:3': 'chiseled_stone_bricks',
        '99:0': 'brown_mushroom_block[north=false,east=false,south=false,west=false,up=false,down=false]',
        '99:1': 'brown_mushroom_block[north=true,east=false,south=false,west=true,up=true,down=false]',
        '99:2': 'brown_mushroom_block[north=true,east=false,south=false,west=false,up=true,down=false]',
        '99:3': 'brown_mushroom_block[north=true,east=true,south=false,west=false,up=true,down=false]',
        '99:4': 'brown_mushroom_block[north=false,east=false,south=false,west=true,up=true,down=false]',
        '99:5': 'brown_mushroom_block[north=false,east=false,south=false,west=false,up=true,down=false]',
        '99:6': 'brown_mushroom_block[north=false,east=true,south=false,west=false,up=true,down=false]',
        '99:7': 'brown_mushroom_block[north=false,east=false,south=true,west=true,up=true,down=false]',
        '99:8': 'brown_mushroom_block[north=false,east=false,south=true,west=false,up=true,down=false]',
        '99:9': 'brown_mushroom_block[north=false,east=true,south=true,west=false,up=true,down=false]',
        '99:10': 'mushroom_stem[north=true,east=true,south=true,west=true,up=false,down=false]',
        '99:14': 'brown_mushroom_block[north=true,east=true,south=true,west=true,up=true,down=true]',
        '99:15': 'mushroom_stem[north=true,east=true,south=true,west=true,up=true,down=true]',
        '100:0': 'red_mushroom_block[north=false,east=false,south=false,west=false,up=false,down=false]',
        '100:1': 'red_mushroom_block[north=true,east=false,south=false,west=true,up=true,down=false]',
        '100:2': 'red_mushroom_block[north=true,east=false,south=false,west=false,up=true,down=false]',
        '100:3': 'red_mushroom_block[north=true,east=true,south=false,west=false,up=true,down=false]',
        '100:4': 'red_mushroom_block[north=false,east=false,south=false,west=true,up=true,down=false]',
        '100:5': 'red_mushroom_block[north=false,east=false,south=false,west=false,up=true,down=false]',
        '100:6': 'red_mushroom_block[north=false,east=true,south=false,west=false,up=true,down=false]',
        '100:7': 'red_mushroom_block[north=false,east=false,south=true,west=true,up=true,down=false]',
        '100:8': 'red_mushroom_block[north=false,east=false,south=true,west=false,up=true,down=false]',
        '100:9': 'red_mushroom_block[north=false,east=true,south=true,west=false,up=true,down=false]',
        '100:10': 'mushroom_stem[north=true,east=true,south=true,west=true,up=false,down=false]',
        '100:14': 'red_mushroom_block[north=true,east=true,south=true,west=true,up=true,down=true]',
        '100:15': 'mushroom_stem[north=true,east=true,south=true,west=true,up=true,down=true]',
        '101:0': 'iron_bars[east=false,south=false,north=false,west=false]',
        '102:0': 'glass_pane[east=false,south=false,north=false,west=false]',
        '103:0': 'melon',
        '104:0': 'pumpkin_stem[age=0]',
        '104:1': 'pumpkin_stem[age=1]',
        '104:2': 'pumpkin_stem[age=2]',
        '104:3': 'pumpkin_stem[age=3]',
        '104:4': 'pumpkin_stem[age=4]',
        '104:5': 'pumpkin_stem[age=5]',
        '104:6': 'pumpkin_stem[age=6]',
        '104:7': 'pumpkin_stem[age=7]',
        '105:0': 'melon_stem[age=0]',
        '105:1': 'melon_stem[age=1]',
        '105:2': 'melon_stem[age=2]',
        '105:3': 'melon_stem[age=3]',
        '105:4': 'melon_stem[age=4]',
        '105:5': 'melon_stem[age=5]',
        '105:6': 'melon_stem[age=6]',
        '105:7': 'melon_stem[age=7]',
        '106:0': 'vine[east=false,south=false,north=false,west=false,up=false]',
        '106:1': 'vine[east=false,south=true,north=false,west=false,up=false]',
        '106:2': 'vine[east=false,south=false,north=false,west=true,up=false]',
        '106:3': 'vine[east=false,south=true,north=false,west=true,up=false]',
        '106:4': 'vine[east=false,south=false,north=true,west=false,up=false]',
        '106:5': 'vine[east=false,south=true,north=true,west=false,up=false]',
        '106:6': 'vine[east=false,south=false,north=true,west=true,up=false]',
        '106:7': 'vine[east=false,south=true,north=true,west=true,up=false]',
        '106:8': 'vine[east=true,south=false,north=false,west=false,up=false]',
        '106:9': 'vine[east=true,south=true,north=false,west=false,up=false]',
        '106:10': 'vine[east=true,south=false,north=false,west=true,up=false]',
        '106:11': 'vine[east=true,south=true,north=false,west=true,up=false]',
        '106:12': 'vine[east=true,south=false,north=true,west=false,up=false]',
        '106:13': 'vine[east=true,south=true,north=true,west=false,up=false]',
        '106:14': 'vine[east=true,south=false,north=true,west=true,up=false]',
        '106:15': 'vine[east=true,south=true,north=true,west=true,up=false]',
        '107:0': 'oak_fence_gate[in_wall=false,powered=false,facing=south,open=false]',
        '107:1': 'oak_fence_gate[in_wall=false,powered=false,facing=west,open=false]',
        '107:2': 'oak_fence_gate[in_wall=false,powered=false,facing=north,open=false]',
        '107:3': 'oak_fence_gate[in_wall=false,powered=false,facing=east,open=false]',
        '107:4': 'oak_fence_gate[in_wall=false,powered=false,facing=south,open=true]',
        '107:5': 'oak_fence_gate[in_wall=false,powered=false,facing=west,open=true]',
        '107:6': 'oak_fence_gate[in_wall=false,powered=false,facing=north,open=true]',
        '107:7': 'oak_fence_gate[in_wall=false,powered=false,facing=east,open=true]',
        '107:8': 'oak_fence_gate[in_wall=false,powered=true,facing=south,open=false]',
        '107:9': 'oak_fence_gate[in_wall=false,powered=true,facing=west,open=false]',
        '107:10': 'oak_fence_gate[in_wall=false,powered=true,facing=north,open=false]',
        '107:11': 'oak_fence_gate[in_wall=false,powered=true,facing=east,open=false]',
        '107:12': 'oak_fence_gate[in_wall=false,powered=true,facing=south,open=true]',
        '107:13': 'oak_fence_gate[in_wall=false,powered=true,facing=west,open=true]',
        '107:14': 'oak_fence_gate[in_wall=false,powered=true,facing=north,open=true]',
        '107:15': 'oak_fence_gate[in_wall=false,powered=true,facing=east,open=true]',
        '108:0': 'brick_stairs[half=bottom,shape=straight,facing=east]',
        '108:1': 'brick_stairs[half=bottom,shape=straight,facing=west]',
        '108:2': 'brick_stairs[half=bottom,shape=straight,facing=south]',
        '108:3': 'brick_stairs[half=bottom,shape=straight,facing=north]',
        '108:4': 'brick_stairs[half=top,shape=straight,facing=east]',
        '108:5': 'brick_stairs[half=top,shape=straight,facing=west]',
        '108:6': 'brick_stairs[half=top,shape=straight,facing=south]',
        '108:7': 'brick_stairs[half=top,shape=straight,facing=north]',
        '109:0': 'stone_brick_stairs[half=bottom,shape=straight,facing=east]',
        '109:1': 'stone_brick_stairs[half=bottom,shape=straight,facing=west]',
        '109:2': 'stone_brick_stairs[half=bottom,shape=straight,facing=south]',
        '109:3': 'stone_brick_stairs[half=bottom,shape=straight,facing=north]',
        '109:4': 'stone_brick_stairs[half=top,shape=straight,facing=east]',
        '109:5': 'stone_brick_stairs[half=top,shape=straight,facing=west]',
        '109:6': 'stone_brick_stairs[half=top,shape=straight,facing=south]',
        '109:7': 'stone_brick_stairs[half=top,shape=straight,facing=north]',
        '110:0': 'mycelium[snowy=false]',
        '111:0': 'lily_pad',
        '112:0': 'nether_bricks',
        '113:0': 'nether_brick_fence[east=false,south=false,north=false,west=false]',
        '114:0': 'nether_brick_stairs[half=bottom,shape=straight,facing=east]',
        '114:1': 'nether_brick_stairs[half=bottom,shape=straight,facing=west]',
        '114:2': 'nether_brick_stairs[half=bottom,shape=straight,facing=south]',
        '114:3': 'nether_brick_stairs[half=bottom,shape=straight,facing=north]',
        '114:4': 'nether_brick_stairs[half=top,shape=straight,facing=east]',
        '114:5': 'nether_brick_stairs[half=top,shape=straight,facing=west]',
        '114:6': 'nether_brick_stairs[half=top,shape=straight,facing=south]',
        '114:7': 'nether_brick_stairs[half=top,shape=straight,facing=north]',
        '115:0': 'nether_wart[age=0]',
        '115:1': 'nether_wart[age=1]',
        '115:2': 'nether_wart[age=2]',
        '115:3': 'nether_wart[age=3]',
        '116:0': 'enchanting_table',
        '117:0': 'brewing_stand[has_bottle_0=false,has_bottle_1=false,has_bottle_2=false]',
        '117:1': 'brewing_stand[has_bottle_0=true,has_bottle_1=false,has_bottle_2=false]',
        '117:2': 'brewing_stand[has_bottle_0=false,has_bottle_1=true,has_bottle_2=false]',
        '117:3': 'brewing_stand[has_bottle_0=true,has_bottle_1=true,has_bottle_2=false]',
        '117:4': 'brewing_stand[has_bottle_0=false,has_bottle_1=false,has_bottle_2=true]',
        '117:5': 'brewing_stand[has_bottle_0=true,has_bottle_1=false,has_bottle_2=true]',
        '117:6': 'brewing_stand[has_bottle_0=false,has_bottle_1=true,has_bottle_2=true]',
        '117:7': 'brewing_stand[has_bottle_0=true,has_bottle_1=true,has_bottle_2=true]',
        '118:0': 'cauldron[level=0]',
        '118:1': 'cauldron[level=1]',
        '118:2': 'cauldron[level=2]',
        '118:3': 'cauldron[level=3]',
        '119:0': 'end_portal',
        '120:0': 'end_portal_frame[eye=false,facing=south]',
        '120:1': 'end_portal_frame[eye=false,facing=west]',
        '120:2': 'end_portal_frame[eye=false,facing=north]',
        '120:3': 'end_portal_frame[eye=false,facing=east]',
        '120:4': 'end_portal_frame[eye=true,facing=south]',
        '120:5': 'end_portal_frame[eye=true,facing=west]',
        '120:6': 'end_portal_frame[eye=true,facing=north]',
        '120:7': 'end_portal_frame[eye=true,facing=east]',
        '121:0': 'end_stone',
        '122:0': 'dragon_egg',
        '123:0': 'redstone_lamp[lit=false]',
        '124:0': 'redstone_lamp[lit=true]',
        '125:0': 'oak_slab[type=double]',
        '125:1': 'spruce_slab[type=double]',
        '125:2': 'birch_slab[type=double]',
        '125:3': 'jungle_slab[type=double]',
        '125:4': 'acacia_slab[type=double]',
        '125:5': 'dark_oak_slab[type=double]',
        '126:0': 'oak_slab[type=bottom]',
        '126:1': 'spruce_slab[type=bottom]',
        '126:2': 'birch_slab[type=bottom]',
        '126:3': 'jungle_slab[type=bottom]',
        '126:4': 'acacia_slab[type=bottom]',
        '126:5': 'dark_oak_slab[type=bottom]',
        '126:8': 'oak_slab[type=top]',
        '126:9': 'spruce_slab[type=top]',
        '126:10': 'birch_slab[type=top]',
        '126:11': 'jungle_slab[type=top]',
        '126:12': 'acacia_slab[type=top]',
        '126:13': 'dark_oak_slab[type=top]',
        '127:0': 'cocoa[facing=south,age=0]',
        '127:1': 'cocoa[facing=west,age=0]',
        '127:2': 'cocoa[facing=north,age=0]',
        '127:3': 'cocoa[facing=east,age=0]',
        '127:4': 'cocoa[facing=south,age=1]',
        '127:5': 'cocoa[facing=west,age=1]',
        '127:6': 'cocoa[facing=north,age=1]',
        '127:7': 'cocoa[facing=east,age=1]',
        '127:8': 'cocoa[facing=south,age=2]',
        '127:9': 'cocoa[facing=west,age=2]',
        '127:10': 'cocoa[facing=north,age=2]',
        '127:11': 'cocoa[facing=east,age=2]',
        '128:0': 'sandstone_stairs[half=bottom,shape=straight,facing=east]',
        '128:1': 'sandstone_stairs[half=bottom,shape=straight,facing=west]',
        '128:2': 'sandstone_stairs[half=bottom,shape=straight,facing=south]',
        '128:3': 'sandstone_stairs[half=bottom,shape=straight,facing=north]',
        '128:4': 'sandstone_stairs[half=top,shape=straight,facing=east]',
        '128:5': 'sandstone_stairs[half=top,shape=straight,facing=west]',
        '128:6': 'sandstone_stairs[half=top,shape=straight,facing=south]',
        '128:7': 'sandstone_stairs[half=top,shape=straight,facing=north]',
        '129:0': 'emerald_ore',
        '130:2': 'ender_chest[facing=north]',
        '130:3': 'ender_chest[facing=south]',
        '130:4': 'ender_chest[facing=west]',
        '130:5': 'ender_chest[facing=east]',
        '131:0': 'tripwire_hook[powered=false,attached=false,facing=south]',
        '131:1': 'tripwire_hook[powered=false,attached=false,facing=west]',
        '131:2': 'tripwire_hook[powered=false,attached=false,facing=north]',
        '131:3': 'tripwire_hook[powered=false,attached=false,facing=east]',
        '131:4': 'tripwire_hook[powered=false,attached=true,facing=south]',
        '131:5': 'tripwire_hook[powered=false,attached=true,facing=west]',
        '131:6': 'tripwire_hook[powered=false,attached=true,facing=north]',
        '131:7': 'tripwire_hook[powered=false,attached=true,facing=east]',
        '131:8': 'tripwire_hook[powered=true,attached=false,facing=south]',
        '131:9': 'tripwire_hook[powered=true,attached=false,facing=west]',
        '131:10': 'tripwire_hook[powered=true,attached=false,facing=north]',
        '131:11': 'tripwire_hook[powered=true,attached=false,facing=east]',
        '131:12': 'tripwire_hook[powered=true,attached=true,facing=south]',
        '131:13': 'tripwire_hook[powered=true,attached=true,facing=west]',
        '131:14': 'tripwire_hook[powered=true,attached=true,facing=north]',
        '131:15': 'tripwire_hook[powered=true,attached=true,facing=east]',
        '132:0': 'tripwire[disarmed=false,east=false,powered=false,south=false,north=false,west=false,attached=false]',
        '132:1': 'tripwire[disarmed=false,east=false,powered=true,south=false,north=false,west=false,attached=false]',
        '132:4': 'tripwire[disarmed=false,east=false,powered=false,south=false,north=false,west=false,attached=true]',
        '132:5': 'tripwire[disarmed=false,east=false,powered=true,south=false,north=false,west=false,attached=true]',
        '132:8': 'tripwire[disarmed=true,east=false,powered=false,south=false,north=false,west=false,attached=false]',
        '132:9': 'tripwire[disarmed=true,east=false,powered=true,south=false,north=false,west=false,attached=false]',
        '132:12': 'tripwire[disarmed=true,east=false,powered=false,south=false,north=false,west=false,attached=true]',
        '132:13': 'tripwire[disarmed=true,east=false,powered=true,south=false,north=false,west=false,attached=true]',
        '133:0': 'emerald_block',
        '134:0': 'spruce_stairs[half=bottom,shape=straight,facing=east]',
        '134:1': 'spruce_stairs[half=bottom,shape=straight,facing=west]',
        '134:2': 'spruce_stairs[half=bottom,shape=straight,facing=south]',
        '134:3': 'spruce_stairs[half=bottom,shape=straight,facing=north]',
        '134:4': 'spruce_stairs[half=top,shape=straight,facing=east]',
        '134:5': 'spruce_stairs[half=top,shape=straight,facing=west]',
        '134:6': 'spruce_stairs[half=top,shape=straight,facing=south]',
        '134:7': 'spruce_stairs[half=top,shape=straight,facing=north]',
        '135:0': 'birch_stairs[half=bottom,shape=straight,facing=east]',
        '135:1': 'birch_stairs[half=bottom,shape=straight,facing=west]',
        '135:2': 'birch_stairs[half=bottom,shape=straight,facing=south]',
        '135:3': 'birch_stairs[half=bottom,shape=straight,facing=north]',
        '135:4': 'birch_stairs[half=top,shape=straight,facing=east]',
        '135:5': 'birch_stairs[half=top,shape=straight,facing=west]',
        '135:6': 'birch_stairs[half=top,shape=straight,facing=south]',
        '135:7': 'birch_stairs[half=top,shape=straight,facing=north]',
        '136:0': 'jungle_stairs[half=bottom,shape=straight,facing=east]',
        '136:1': 'jungle_stairs[half=bottom,shape=straight,facing=west]',
        '136:2': 'jungle_stairs[half=bottom,shape=straight,facing=south]',
        '136:3': 'jungle_stairs[half=bottom,shape=straight,facing=north]',
        '136:4': 'jungle_stairs[half=top,shape=straight,facing=east]',
        '136:5': 'jungle_stairs[half=top,shape=straight,facing=west]',
        '136:6': 'jungle_stairs[half=top,shape=straight,facing=south]',
        '136:7': 'jungle_stairs[half=top,shape=straight,facing=north]',
        '137:0': 'command_block[conditional=false,facing=down]',
        '137:1': 'command_block[conditional=false,facing=up]',
        '137:2': 'command_block[conditional=false,facing=north]',
        '137:3': 'command_block[conditional=false,facing=south]',
        '137:4': 'command_block[conditional=false,facing=west]',
        '137:5': 'command_block[conditional=false,facing=east]',
        '137:8': 'command_block[conditional=true,facing=down]',
        '137:9': 'command_block[conditional=true,facing=up]',
        '137:10': 'command_block[conditional=true,facing=north]',
        '137:11': 'command_block[conditional=true,facing=south]',
        '137:12': 'command_block[conditional=true,facing=west]',
        '137:13': 'command_block[conditional=true,facing=east]',
        '138:0': 'beacon',
        '139:0': 'cobblestone_wall[east=false,south=false,north=false,west=false,up=false]',
        '139:1': 'mossy_cobblestone_wall[east=false,south=false,north=false,west=false,up=false]',
        '140:0': 'flower_pot',
        '140:1': 'potted_poppy',
        '140:2': 'potted_dandelion',
        '140:3': 'potted_oak_sapling',
        '140:4': 'potted_spruce_sapling',
        '140:5': 'potted_birch_sapling',
        '140:6': 'potted_jungle_sapling',
        '140:7': 'potted_red_mushroom',
        '140:8': 'potted_brown_mushroom',
        '140:9': 'potted_cactus',
        '140:10': 'potted_dead_bush',
        '140:11': 'potted_fern',
        '140:12': 'potted_acacia_sapling',
        '140:13': 'potted_dark_oak_sapling',
        '140:14': 'potted_blue_orchid',
        '140:15': 'potted_allium',
        '141:0': 'carrots[age=0]',
        '141:1': 'carrots[age=1]',
        '141:2': 'carrots[age=2]',
        '141:3': 'carrots[age=3]',
        '141:4': 'carrots[age=4]',
        '141:5': 'carrots[age=5]',
        '141:6': 'carrots[age=6]',
        '141:7': 'carrots[age=7]',
        '142:0': 'potatoes[age=0]',
        '142:1': 'potatoes[age=1]',
        '142:2': 'potatoes[age=2]',
        '142:3': 'potatoes[age=3]',
        '142:4': 'potatoes[age=4]',
        '142:5': 'potatoes[age=5]',
        '142:6': 'potatoes[age=6]',
        '142:7': 'potatoes[age=7]',
        '143:0': 'oak_button[powered=false,facing=east,face=ceiling]',
        '143:1': 'oak_button[powered=false,facing=east,face=wall]',
        '143:2': 'oak_button[powered=false,facing=west,face=wall]',
        '143:3': 'oak_button[powered=false,facing=south,face=wall]',
        '143:4': 'oak_button[powered=false,facing=north,face=wall]',
        '143:5': 'oak_button[powered=false,facing=east,face=floor]',
        '143:8': 'oak_button[powered=true,facing=south,face=ceiling]',
        '143:9': 'oak_button[powered=true,facing=east,face=wall]',
        '143:10': 'oak_button[powered=true,facing=west,face=wall]',
        '143:11': 'oak_button[powered=true,facing=south,face=wall]',
        '143:12': 'oak_button[powered=true,facing=north,face=wall]',
        '143:13': 'oak_button[powered=true,facing=south,face=floor]',
        '144:0': 'skeleton_skull[rotation=0]',
        '144:1': 'skeleton_skull[rotation=4]',
        '144:2': 'skeleton_wall_skull[facing=north]',
        '144:3': 'skeleton_wall_skull[facing=south]',
        '144:4': 'skeleton_wall_skull[facing=west]',
        '144:5': 'skeleton_wall_skull[facing=east]',
        '144:8': 'skeleton_skull[rotation=8]',
        '144:9': 'skeleton_skull[rotation=12]',
        '144:10': 'skeleton_wall_skull[facing=north]',
        '144:11': 'skeleton_wall_skull[facing=south]',
        '144:12': 'skeleton_wall_skull[facing=west]',
        '144:13': 'skeleton_wall_skull[facing=east]',
        '145:0': 'anvil[facing=south]',
        '145:1': 'anvil[facing=west]',
        '145:2': 'anvil[facing=north]',
        '145:3': 'anvil[facing=east]',
        '145:4': 'chipped_anvil[facing=south]',
        '145:5': 'chipped_anvil[facing=west]',
        '145:6': 'chipped_anvil[facing=north]',
        '145:7': 'chipped_anvil[facing=east]',
        '145:8': 'damaged_anvil[facing=south]',
        '145:9': 'damaged_anvil[facing=west]',
        '145:10': 'damaged_anvil[facing=north]',
        '145:11': 'damaged_anvil[facing=east]',
        '146:2': 'trapped_chest[facing=north,type=single]',
        '146:3': 'trapped_chest[facing=south,type=single]',
        '146:4': 'trapped_chest[facing=west,type=single]',
        '146:5': 'trapped_chest[facing=east,type=single]',
        '147:0': 'light_weighted_pressure_plate[power=0]',
        '147:1': 'light_weighted_pressure_plate[power=1]',
        '147:2': 'light_weighted_pressure_plate[power=2]',
        '147:3': 'light_weighted_pressure_plate[power=3]',
        '147:4': 'light_weighted_pressure_plate[power=4]',
        '147:5': 'light_weighted_pressure_plate[power=5]',
        '147:6': 'light_weighted_pressure_plate[power=6]',
        '147:7': 'light_weighted_pressure_plate[power=7]',
        '147:8': 'light_weighted_pressure_plate[power=8]',
        '147:9': 'light_weighted_pressure_plate[power=9]',
        '147:10': 'light_weighted_pressure_plate[power=10]',
        '147:11': 'light_weighted_pressure_plate[power=11]',
        '147:12': 'light_weighted_pressure_plate[power=12]',
        '147:13': 'light_weighted_pressure_plate[power=13]',
        '147:14': 'light_weighted_pressure_plate[power=14]',
        '147:15': 'light_weighted_pressure_plate[power=15]',
        '148:0': 'heavy_weighted_pressure_plate[power=0]',
        '148:1': 'heavy_weighted_pressure_plate[power=1]',
        '148:2': 'heavy_weighted_pressure_plate[power=2]',
        '148:3': 'heavy_weighted_pressure_plate[power=3]',
        '148:4': 'heavy_weighted_pressure_plate[power=4]',
        '148:5': 'heavy_weighted_pressure_plate[power=5]',
        '148:6': 'heavy_weighted_pressure_plate[power=6]',
        '148:7': 'heavy_weighted_pressure_plate[power=7]',
        '148:8': 'heavy_weighted_pressure_plate[power=8]',
        '148:9': 'heavy_weighted_pressure_plate[power=9]',
        '148:10': 'heavy_weighted_pressure_plate[power=10]',
        '148:11': 'heavy_weighted_pressure_plate[power=11]',
        '148:12': 'heavy_weighted_pressure_plate[power=12]',
        '148:13': 'heavy_weighted_pressure_plate[power=13]',
        '148:14': 'heavy_weighted_pressure_plate[power=14]',
        '148:15': 'heavy_weighted_pressure_plate[power=15]',
        '149:0': 'comparator[mode=compare,powered=false,facing=south]',
        '149:1': 'comparator[mode=compare,powered=false,facing=west]',
        '149:2': 'comparator[mode=compare,powered=false,facing=north]',
        '149:3': 'comparator[mode=compare,powered=false,facing=east]',
        '149:4': 'comparator[mode=subtract,powered=false,facing=south]',
        '149:5': 'comparator[mode=subtract,powered=false,facing=west]',
        '149:6': 'comparator[mode=subtract,powered=false,facing=north]',
        '149:7': 'comparator[mode=subtract,powered=false,facing=east]',
        '149:8': 'comparator[mode=compare,powered=false,facing=south]',
        '149:9': 'comparator[mode=compare,powered=false,facing=west]',
        '149:10': 'comparator[mode=compare,powered=false,facing=north]',
        '149:11': 'comparator[mode=compare,powered=false,facing=east]',
        '149:12': 'comparator[mode=subtract,powered=false,facing=south]',
        '149:13': 'comparator[mode=subtract,powered=false,facing=west]',
        '149:14': 'comparator[mode=subtract,powered=false,facing=north]',
        '149:15': 'comparator[mode=subtract,powered=false,facing=east]',
        '150:0': 'comparator[mode=compare,powered=true,facing=south]',
        '150:1': 'comparator[mode=compare,powered=true,facing=west]',
        '150:2': 'comparator[mode=compare,powered=true,facing=north]',
        '150:3': 'comparator[mode=compare,powered=true,facing=east]',
        '150:4': 'comparator[mode=subtract,powered=true,facing=south]',
        '150:5': 'comparator[mode=subtract,powered=true,facing=west]',
        '150:6': 'comparator[mode=subtract,powered=true,facing=north]',
        '150:7': 'comparator[mode=subtract,powered=true,facing=east]',
        '150:8': 'comparator[mode=compare,powered=true,facing=south]',
        '150:9': 'comparator[mode=compare,powered=true,facing=west]',
        '150:10': 'comparator[mode=compare,powered=true,facing=north]',
        '150:11': 'comparator[mode=compare,powered=true,facing=east]',
        '150:12': 'comparator[mode=subtract,powered=true,facing=south]',
        '150:13': 'comparator[mode=subtract,powered=true,facing=west]',
        '150:14': 'comparator[mode=subtract,powered=true,facing=north]',
        '150:15': 'comparator[mode=subtract,powered=true,facing=east]',
        '151:0': 'daylight_detector[inverted=false,power=0]',
        '151:1': 'daylight_detector[inverted=false,power=1]',
        '151:2': 'daylight_detector[inverted=false,power=2]',
        '151:3': 'daylight_detector[inverted=false,power=3]',
        '151:4': 'daylight_detector[inverted=false,power=4]',
        '151:5': 'daylight_detector[inverted=false,power=5]',
        '151:6': 'daylight_detector[inverted=false,power=6]',
        '151:7': 'daylight_detector[inverted=false,power=7]',
        '151:8': 'daylight_detector[inverted=false,power=8]',
        '151:9': 'daylight_detector[inverted=false,power=9]',
        '151:10': 'daylight_detector[inverted=false,power=10]',
        '151:11': 'daylight_detector[inverted=false,power=11]',
        '151:12': 'daylight_detector[inverted=false,power=12]',
        '151:13': 'daylight_detector[inverted=false,power=13]',
        '151:14': 'daylight_detector[inverted=false,power=14]',
        '151:15': 'daylight_detector[inverted=false,power=15]',
        '152:0': 'redstone_block',
        '153:0': 'nether_quartz_ore',
        '154:0': 'hopper[facing=down,enabled=true]',
        '154:2': 'hopper[facing=north,enabled=true]',
        '154:3': 'hopper[facing=south,enabled=true]',
        '154:4': 'hopper[facing=west,enabled=true]',
        '154:5': 'hopper[facing=east,enabled=true]',
        '154:8': 'hopper[facing=down,enabled=false]',
        '154:10': 'hopper[facing=north,enabled=false]',
        '154:11': 'hopper[facing=south,enabled=false]',
        '154:12': 'hopper[facing=west,enabled=false]',
        '154:13': 'hopper[facing=east,enabled=false]',
        '155:0': 'quartz_block',
        '155:1': 'chiseled_quartz_block',
        '155:2': 'quartz_pillar[axis=y]',
        '155:3': 'quartz_pillar[axis=x]',
        '155:4': 'quartz_pillar[axis=z]',
        '155:6': 'quartz_pillar[axis=x]',
        '155:10': 'quartz_pillar[axis=z]',
        '156:0': 'quartz_stairs[half=bottom,shape=straight,facing=east]',
        '156:1': 'quartz_stairs[half=bottom,shape=straight,facing=west]',
        '156:2': 'quartz_stairs[half=bottom,shape=straight,facing=south]',
        '156:3': 'quartz_stairs[half=bottom,shape=straight,facing=north]',
        '156:4': 'quartz_stairs[half=top,shape=straight,facing=east]',
        '156:5': 'quartz_stairs[half=top,shape=straight,facing=west]',
        '156:6': 'quartz_stairs[half=top,shape=straight,facing=south]',
        '156:7': 'quartz_stairs[half=top,shape=straight,facing=north]',
        '157:0': 'activator_rail[shape=north_south,powered=false]',
        '157:1': 'activator_rail[shape=east_west,powered=false]',
        '157:2': 'activator_rail[shape=ascending_east,powered=false]',
        '157:3': 'activator_rail[shape=ascending_west,powered=false]',
        '157:4': 'activator_rail[shape=ascending_north,powered=false]',
        '157:5': 'activator_rail[shape=ascending_south,powered=false]',
        '157:8': 'activator_rail[shape=north_south,powered=true]',
        '157:9': 'activator_rail[shape=east_west,powered=true]',
        '157:10': 'activator_rail[shape=ascending_east,powered=true]',
        '157:11': 'activator_rail[shape=ascending_west,powered=true]',
        '157:12': 'activator_rail[shape=ascending_north,powered=true]',
        '157:13': 'activator_rail[shape=ascending_south,powered=true]',
        '158:0': 'dropper[triggered=false,facing=down]',
        '158:1': 'dropper[triggered=false,facing=up]',
        '158:2': 'dropper[triggered=false,facing=north]',
        '158:3': 'dropper[triggered=false,facing=south]',
        '158:4': 'dropper[triggered=false,facing=west]',
        '158:5': 'dropper[triggered=false,facing=east]',
        '158:8': 'dropper[triggered=true,facing=down]',
        '158:9': 'dropper[triggered=true,facing=up]',
        '158:10': 'dropper[triggered=true,facing=north]',
        '158:11': 'dropper[triggered=true,facing=south]',
        '158:12': 'dropper[triggered=true,facing=west]',
        '158:13': 'dropper[triggered=true,facing=east]',
        '159:0': 'white_terracotta',
        '159:1': 'orange_terracotta',
        '159:2': 'magenta_terracotta',
        '159:3': 'light_blue_terracotta',
        '159:4': 'yellow_terracotta',
        '159:5': 'lime_terracotta',
        '159:6': 'pink_terracotta',
        '159:7': 'gray_terracotta',
        '159:8': 'light_gray_terracotta',
        '159:9': 'cyan_terracotta',
        '159:10': 'purple_terracotta',
        '159:11': 'blue_terracotta',
        '159:12': 'brown_terracotta',
        '159:13': 'green_terracotta',
        '159:14': 'red_terracotta',
        '159:15': 'black_terracotta',
        '160:0': 'white_stained_glass_pane[east=false,south=false,north=false,west=false]',
        '160:1': 'orange_stained_glass_pane[east=false,south=false,north=false,west=false]',
        '160:2': 'magenta_stained_glass_pane[east=false,south=false,north=false,west=false]',
        '160:3': 'light_blue_stained_glass_pane[east=false,south=false,north=false,west=false]',
        '160:4': 'yellow_stained_glass_pane[east=false,south=false,north=false,west=false]',
        '160:5': 'lime_stained_glass_pane[east=false,south=false,north=false,west=false]',
        '160:6': 'pink_stained_glass_pane[east=false,south=false,north=false,west=false]',
        '160:7': 'gray_stained_glass_pane[east=false,south=false,north=false,west=false]',
        '160:8': 'light_gray_stained_glass_pane[east=false,south=false,north=false,west=false]',
        '160:9': 'cyan_stained_glass_pane[east=false,south=false,north=false,west=false]',
        '160:10': 'purple_stained_glass_pane[east=false,south=false,north=false,west=false]',
        '160:11': 'blue_stained_glass_pane[east=false,south=false,north=false,west=false]',
        '160:12': 'brown_stained_glass_pane[east=false,south=false,north=false,west=false]',
        '160:13': 'green_stained_glass_pane[east=false,south=false,north=false,west=false]',
        '160:14': 'red_stained_glass_pane[east=false,south=false,north=false,west=false]',
        '160:15': 'black_stained_glass_pane[east=false,south=false,north=false,west=false]',
        '161:0': 'acacia_leaves[persistent=false,distance=1]',
        '161:1': 'dark_oak_leaves[persistent=false,distance=1]',
        '161:4': 'acacia_leaves[persistent=true,distance=1]',
        '161:5': 'dark_oak_leaves[persistent=true,distance=1]',
        '161:8': 'acacia_leaves[persistent=false,distance=1]',
        '161:9': 'dark_oak_leaves[persistent=false,distance=1]',
        '161:12': 'acacia_leaves[persistent=true,distance=1]',
        '161:13': 'dark_oak_leaves[persistent=true,distance=1]',
        '162:0': 'acacia_log[axis=y]',
        '162:1': 'dark_oak_log[axis=y]',
        '162:4': 'acacia_log[axis=x]',
        '162:5': 'dark_oak_log[axis=x]',
        '162:8': 'acacia_log[axis=z]',
        '162:9': 'dark_oak_log[axis=z]',
        '162:12': 'acacia_wood',
        '162:13': 'dark_oak_wood',
        '163:0': 'acacia_stairs[half=bottom,shape=straight,facing=east]',
        '163:1': 'acacia_stairs[half=bottom,shape=straight,facing=west]',
        '163:2': 'acacia_stairs[half=bottom,shape=straight,facing=south]',
        '163:3': 'acacia_stairs[half=bottom,shape=straight,facing=north]',
        '163:4': 'acacia_stairs[half=top,shape=straight,facing=east]',
        '163:5': 'acacia_stairs[half=top,shape=straight,facing=west]',
        '163:6': 'acacia_stairs[half=top,shape=straight,facing=south]',
        '163:7': 'acacia_stairs[half=top,shape=straight,facing=north]',
        '164:0': 'dark_oak_stairs[half=bottom,shape=straight,facing=east]',
        '164:1': 'dark_oak_stairs[half=bottom,shape=straight,facing=west]',
        '164:2': 'dark_oak_stairs[half=bottom,shape=straight,facing=south]',
        '164:3': 'dark_oak_stairs[half=bottom,shape=straight,facing=north]',
        '164:4': 'dark_oak_stairs[half=top,shape=straight,facing=east]',
        '164:5': 'dark_oak_stairs[half=top,shape=straight,facing=west]',
        '164:6': 'dark_oak_stairs[half=top,shape=straight,facing=south]',
        '164:7': 'dark_oak_stairs[half=top,shape=straight,facing=north]',
        '165:0': 'slime_block',
        '166:0': 'barrier',
        '167:0': 'iron_trapdoor[half=bottom,facing=north,open=false]',
        '167:1': 'iron_trapdoor[half=bottom,facing=south,open=false]',
        '167:2': 'iron_trapdoor[half=bottom,facing=west,open=false]',
        '167:3': 'iron_trapdoor[half=bottom,facing=east,open=false]',
        '167:4': 'iron_trapdoor[half=bottom,facing=north,open=true]',
        '167:5': 'iron_trapdoor[half=bottom,facing=south,open=true]',
        '167:6': 'iron_trapdoor[half=bottom,facing=west,open=true]',
        '167:7': 'iron_trapdoor[half=bottom,facing=east,open=true]',
        '167:8': 'iron_trapdoor[half=top,facing=north,open=false]',
        '167:9': 'iron_trapdoor[half=top,facing=south,open=false]',
        '167:10': 'iron_trapdoor[half=top,facing=west,open=false]',
        '167:11': 'iron_trapdoor[half=top,facing=east,open=false]',
        '167:12': 'iron_trapdoor[half=top,facing=north,open=true]',
        '167:13': 'iron_trapdoor[half=top,facing=south,open=true]',
        '167:14': 'iron_trapdoor[half=top,facing=west,open=true]',
        '167:15': 'iron_trapdoor[half=top,facing=east,open=true]',
        '168:0': 'prismarine',
        '168:1': 'prismarine_bricks',
        '168:2': 'dark_prismarine',
        '169:0': 'sea_lantern',
        '170:0': 'hay_block[axis=y]',
        '170:4': 'hay_block[axis=x]',
        '170:8': 'hay_block[axis=z]',
        '171:0': 'white_carpet',
        '171:1': 'orange_carpet',
        '171:2': 'magenta_carpet',
        '171:3': 'light_blue_carpet',
        '171:4': 'yellow_carpet',
        '171:5': 'lime_carpet',
        '171:6': 'pink_carpet',
        '171:7': 'gray_carpet',
        '171:8': 'light_gray_carpet',
        '171:9': 'cyan_carpet',
        '171:10': 'purple_carpet',
        '171:11': 'blue_carpet',
        '171:12': 'brown_carpet',
        '171:13': 'green_carpet',
        '171:14': 'red_carpet',
        '171:15': 'black_carpet',
        '172:0': 'terracotta',
        '173:0': 'coal_block',
        '174:0': 'packed_ice',
        '175:0': 'sunflower[half=lower]',
        '175:1': 'lilac[half=lower]',
        '175:2': 'tall_grass[half=lower]',
        '175:3': 'large_fern[half=lower]',
        '175:4': 'rose_bush[half=lower]',
        '175:5': 'peony[half=lower]',
        '175:8': 'sunflower[half=upper]',
        '175:9': 'lilac[half=upper]',
        '175:10': 'tall_grass[half=upper]',
        '175:11': 'large_fern[half=upper]',
        '175:12': 'rose_bush[half=upper]',
        '175:13': 'peony[half=upper]',
        '176:0': 'white_banner[rotation=0]',
        '176:1': 'white_banner[rotation=1]',
        '176:2': 'white_banner[rotation=2]',
        '176:3': 'white_banner[rotation=3]',
        '176:4': 'white_banner[rotation=4]',
        '176:5': 'white_banner[rotation=5]',
        '176:6': 'white_banner[rotation=6]',
        '176:7': 'white_banner[rotation=7]',
        '176:8': 'white_banner[rotation=8]',
        '176:9': 'white_banner[rotation=9]',
        '176:10': 'white_banner[rotation=10]',
        '176:11': 'white_banner[rotation=11]',
        '176:12': 'white_banner[rotation=12]',
        '176:13': 'white_banner[rotation=13]',
        '176:14': 'white_banner[rotation=14]',
        '176:15': 'white_banner[rotation=15]',
        '177:2': 'white_wall_banner[facing=north]',
        '177:3': 'white_wall_banner[facing=south]',
        '177:4': 'white_wall_banner[facing=west]',
        '177:5': 'white_wall_banner[facing=east]',
        '178:0': 'daylight_detector[inverted=true,power=0]',
        '178:1': 'daylight_detector[inverted=true,power=1]',
        '178:2': 'daylight_detector[inverted=true,power=2]',
        '178:3': 'daylight_detector[inverted=true,power=3]',
        '178:4': 'daylight_detector[inverted=true,power=4]',
        '178:5': 'daylight_detector[inverted=true,power=5]',
        '178:6': 'daylight_detector[inverted=true,power=6]',
        '178:7': 'daylight_detector[inverted=true,power=7]',
        '178:8': 'daylight_detector[inverted=true,power=8]',
        '178:9': 'daylight_detector[inverted=true,power=9]',
        '178:10': 'daylight_detector[inverted=true,power=10]',
        '178:11': 'daylight_detector[inverted=true,power=11]',
        '178:12': 'daylight_detector[inverted=true,power=12]',
        '178:13': 'daylight_detector[inverted=true,power=13]',
        '178:14': 'daylight_detector[inverted=true,power=14]',
        '178:15': 'daylight_detector[inverted=true,power=15]',
        '179:0': 'red_sandstone',
        '179:1': 'chiseled_red_sandstone',
        '179:2': 'cut_red_sandstone',
        '180:0': 'red_sandstone_stairs[half=bottom,shape=straight,facing=east]',
        '180:1': 'red_sandstone_stairs[half=bottom,shape=straight,facing=west]',
        '180:2': 'red_sandstone_stairs[half=bottom,shape=straight,facing=south]',
        '180:3': 'red_sandstone_stairs[half=bottom,shape=straight,facing=north]',
        '180:4': 'red_sandstone_stairs[half=top,shape=straight,facing=east]',
        '180:5': 'red_sandstone_stairs[half=top,shape=straight,facing=west]',
        '180:6': 'red_sandstone_stairs[half=top,shape=straight,facing=south]',
        '180:7': 'red_sandstone_stairs[half=top,shape=straight,facing=north]',
        '181:0': 'red_sandstone_slab[type=double]',
        '181:8': 'smooth_red_sandstone',
        '182:0': 'red_sandstone_slab[type=bottom]',
        '182:8': 'red_sandstone_slab[type=top]',
        '183:0': 'spruce_fence_gate[in_wall=false,powered=false,facing=south,open=false]',
        '183:1': 'spruce_fence_gate[in_wall=false,powered=false,facing=west,open=false]',
        '183:2': 'spruce_fence_gate[in_wall=false,powered=false,facing=north,open=false]',
        '183:3': 'spruce_fence_gate[in_wall=false,powered=false,facing=east,open=false]',
        '183:4': 'spruce_fence_gate[in_wall=false,powered=false,facing=south,open=true]',
        '183:5': 'spruce_fence_gate[in_wall=false,powered=false,facing=west,open=true]',
        '183:6': 'spruce_fence_gate[in_wall=false,powered=false,facing=north,open=true]',
        '183:7': 'spruce_fence_gate[in_wall=false,powered=false,facing=east,open=true]',
        '183:8': 'spruce_fence_gate[in_wall=false,powered=true,facing=south,open=false]',
        '183:9': 'spruce_fence_gate[in_wall=false,powered=true,facing=west,open=false]',
        '183:10': 'spruce_fence_gate[in_wall=false,powered=true,facing=north,open=false]',
        '183:11': 'spruce_fence_gate[in_wall=false,powered=true,facing=east,open=false]',
        '183:12': 'spruce_fence_gate[in_wall=false,powered=true,facing=south,open=true]',
        '183:13': 'spruce_fence_gate[in_wall=false,powered=true,facing=west,open=true]',
        '183:14': 'spruce_fence_gate[in_wall=false,powered=true,facing=north,open=true]',
        '183:15': 'spruce_fence_gate[in_wall=false,powered=true,facing=east,open=true]',
        '184:0': 'birch_fence_gate[in_wall=false,powered=false,facing=south,open=false]',
        '184:1': 'birch_fence_gate[in_wall=false,powered=false,facing=west,open=false]',
        '184:2': 'birch_fence_gate[in_wall=false,powered=false,facing=north,open=false]',
        '184:3': 'birch_fence_gate[in_wall=false,powered=false,facing=east,open=false]',
        '184:4': 'birch_fence_gate[in_wall=false,powered=false,facing=south,open=true]',
        '184:5': 'birch_fence_gate[in_wall=false,powered=false,facing=west,open=true]',
        '184:6': 'birch_fence_gate[in_wall=false,powered=false,facing=north,open=true]',
        '184:7': 'birch_fence_gate[in_wall=false,powered=false,facing=east,open=true]',
        '184:8': 'birch_fence_gate[in_wall=false,powered=true,facing=south,open=false]',
        '184:9': 'birch_fence_gate[in_wall=false,powered=true,facing=west,open=false]',
        '184:10': 'birch_fence_gate[in_wall=false,powered=true,facing=north,open=false]',
        '184:11': 'birch_fence_gate[in_wall=false,powered=true,facing=east,open=false]',
        '184:12': 'birch_fence_gate[in_wall=false,powered=true,facing=south,open=true]',
        '184:13': 'birch_fence_gate[in_wall=false,powered=true,facing=west,open=true]',
        '184:14': 'birch_fence_gate[in_wall=false,powered=true,facing=north,open=true]',
        '184:15': 'birch_fence_gate[in_wall=false,powered=true,facing=east,open=true]',
        '185:0': 'jungle_fence_gate[in_wall=false,powered=false,facing=south,open=false]',
        '185:1': 'jungle_fence_gate[in_wall=false,powered=false,facing=west,open=false]',
        '185:2': 'jungle_fence_gate[in_wall=false,powered=false,facing=north,open=false]',
        '185:3': 'jungle_fence_gate[in_wall=false,powered=false,facing=east,open=false]',
        '185:4': 'jungle_fence_gate[in_wall=false,powered=false,facing=south,open=true]',
        '185:5': 'jungle_fence_gate[in_wall=false,powered=false,facing=west,open=true]',
        '185:6': 'jungle_fence_gate[in_wall=false,powered=false,facing=north,open=true]',
        '185:7': 'jungle_fence_gate[in_wall=false,powered=false,facing=east,open=true]',
        '185:8': 'jungle_fence_gate[in_wall=false,powered=true,facing=south,open=false]',
        '185:9': 'jungle_fence_gate[in_wall=false,powered=true,facing=west,open=false]',
        '185:10': 'jungle_fence_gate[in_wall=false,powered=true,facing=north,open=false]',
        '185:11': 'jungle_fence_gate[in_wall=false,powered=true,facing=east,open=false]',
        '185:12': 'jungle_fence_gate[in_wall=false,powered=true,facing=south,open=true]',
        '185:13': 'jungle_fence_gate[in_wall=false,powered=true,facing=west,open=true]',
        '185:14': 'jungle_fence_gate[in_wall=false,powered=true,facing=north,open=true]',
        '185:15': 'jungle_fence_gate[in_wall=false,powered=true,facing=east,open=true]',
        '186:0': 'dark_oak_fence_gate[in_wall=false,powered=false,facing=south,open=false]',
        '186:1': 'dark_oak_fence_gate[in_wall=false,powered=false,facing=west,open=false]',
        '186:2': 'dark_oak_fence_gate[in_wall=false,powered=false,facing=north,open=false]',
        '186:3': 'dark_oak_fence_gate[in_wall=false,powered=false,facing=east,open=false]',
        '186:4': 'dark_oak_fence_gate[in_wall=false,powered=false,facing=south,open=true]',
        '186:5': 'dark_oak_fence_gate[in_wall=false,powered=false,facing=west,open=true]',
        '186:6': 'dark_oak_fence_gate[in_wall=false,powered=false,facing=north,open=true]',
        '186:7': 'dark_oak_fence_gate[in_wall=false,powered=false,facing=east,open=true]',
        '186:8': 'dark_oak_fence_gate[in_wall=false,powered=true,facing=south,open=false]',
        '186:9': 'dark_oak_fence_gate[in_wall=false,powered=true,facing=west,open=false]',
        '186:10': 'dark_oak_fence_gate[in_wall=false,powered=true,facing=north,open=false]',
        '186:11': 'dark_oak_fence_gate[in_wall=false,powered=true,facing=east,open=false]',
        '186:12': 'dark_oak_fence_gate[in_wall=false,powered=true,facing=south,open=true]',
        '186:13': 'dark_oak_fence_gate[in_wall=false,powered=true,facing=west,open=true]',
        '186:14': 'dark_oak_fence_gate[in_wall=false,powered=true,facing=north,open=true]',
        '186:15': 'dark_oak_fence_gate[in_wall=false,powered=true,facing=east,open=true]',
        '187:0': 'acacia_fence_gate[in_wall=false,powered=false,facing=south,open=false]',
        '187:1': 'acacia_fence_gate[in_wall=false,powered=false,facing=west,open=false]',
        '187:2': 'acacia_fence_gate[in_wall=false,powered=false,facing=north,open=false]',
        '187:3': 'acacia_fence_gate[in_wall=false,powered=false,facing=east,open=false]',
        '187:4': 'acacia_fence_gate[in_wall=false,powered=false,facing=south,open=true]',
        '187:5': 'acacia_fence_gate[in_wall=false,powered=false,facing=west,open=true]',
        '187:6': 'acacia_fence_gate[in_wall=false,powered=false,facing=north,open=true]',
        '187:7': 'acacia_fence_gate[in_wall=false,powered=false,facing=east,open=true]',
        '187:8': 'acacia_fence_gate[in_wall=false,powered=true,facing=south,open=false]',
        '187:9': 'acacia_fence_gate[in_wall=false,powered=true,facing=west,open=false]',
        '187:10': 'acacia_fence_gate[in_wall=false,powered=true,facing=north,open=false]',
        '187:11': 'acacia_fence_gate[in_wall=false,powered=true,facing=east,open=false]',
        '187:12': 'acacia_fence_gate[in_wall=false,powered=true,facing=south,open=true]',
        '187:13': 'acacia_fence_gate[in_wall=false,powered=true,facing=west,open=true]',
        '187:14': 'acacia_fence_gate[in_wall=false,powered=true,facing=north,open=true]',
        '187:15': 'acacia_fence_gate[in_wall=false,powered=true,facing=east,open=true]',
        '188:0': 'spruce_fence[east=false,south=false,north=false,west=false]',
        '189:0': 'birch_fence[east=false,south=false,north=false,west=false]',
        '190:0': 'jungle_fence[east=false,south=false,north=false,west=false]',
        '191:0': 'dark_oak_fence[east=false,south=false,north=false,west=false]',
        '192:0': 'acacia_fence[east=false,south=false,north=false,west=false]',
        '193:0': 'spruce_door[hinge=right,half=lower,powered=false,facing=east,open=false]',
        '193:1': 'spruce_door[hinge=right,half=lower,powered=false,facing=south,open=false]',
        '193:2': 'spruce_door[hinge=right,half=lower,powered=false,facing=west,open=false]',
        '193:3': 'spruce_door[hinge=right,half=lower,powered=false,facing=north,open=false]',
        '193:4': 'spruce_door[hinge=right,half=lower,powered=false,facing=east,open=true]',
        '193:5': 'spruce_door[hinge=right,half=lower,powered=false,facing=south,open=true]',
        '193:6': 'spruce_door[hinge=right,half=lower,powered=false,facing=west,open=true]',
        '193:7': 'spruce_door[hinge=right,half=lower,powered=false,facing=north,open=true]',
        '193:8': 'spruce_door[hinge=left,half=upper,powered=false,facing=east,open=false]',
        '193:9': 'spruce_door[hinge=right,half=upper,powered=false,facing=east,open=false]',
        '193:10': 'spruce_door[hinge=left,half=upper,powered=true,facing=east,open=false]',
        '193:11': 'spruce_door[hinge=right,half=upper,powered=true,facing=east,open=false]',
        '194:0': 'birch_door[hinge=right,half=lower,powered=false,facing=east,open=false]',
        '194:1': 'birch_door[hinge=right,half=lower,powered=false,facing=south,open=false]',
        '194:2': 'birch_door[hinge=right,half=lower,powered=false,facing=west,open=false]',
        '194:3': 'birch_door[hinge=right,half=lower,powered=false,facing=north,open=false]',
        '194:4': 'birch_door[hinge=right,half=lower,powered=false,facing=east,open=true]',
        '194:5': 'birch_door[hinge=right,half=lower,powered=false,facing=south,open=true]',
        '194:6': 'birch_door[hinge=right,half=lower,powered=false,facing=west,open=true]',
        '194:7': 'birch_door[hinge=right,half=lower,powered=false,facing=north,open=true]',
        '194:8': 'birch_door[hinge=left,half=upper,powered=false,facing=east,open=false]',
        '194:9': 'birch_door[hinge=right,half=upper,powered=false,facing=east,open=false]',
        '194:10': 'birch_door[hinge=left,half=upper,powered=true,facing=east,open=false]',
        '194:11': 'birch_door[hinge=right,half=upper,powered=true,facing=east,open=false]',
        '195:0': 'jungle_door[hinge=right,half=lower,powered=false,facing=east,open=false]',
        '195:1': 'jungle_door[hinge=right,half=lower,powered=false,facing=south,open=false]',
        '195:2': 'jungle_door[hinge=right,half=lower,powered=false,facing=west,open=false]',
        '195:3': 'jungle_door[hinge=right,half=lower,powered=false,facing=north,open=false]',
        '195:4': 'jungle_door[hinge=right,half=lower,powered=false,facing=east,open=true]',
        '195:5': 'jungle_door[hinge=right,half=lower,powered=false,facing=south,open=true]',
        '195:6': 'jungle_door[hinge=right,half=lower,powered=false,facing=west,open=true]',
        '195:7': 'jungle_door[hinge=right,half=lower,powered=false,facing=north,open=true]',
        '195:8': 'jungle_door[hinge=left,half=upper,powered=false,facing=east,open=false]',
        '195:9': 'jungle_door[hinge=right,half=upper,powered=false,facing=east,open=false]',
        '195:10': 'jungle_door[hinge=left,half=upper,powered=true,facing=east,open=false]',
        '195:11': 'jungle_door[hinge=right,half=upper,powered=true,facing=east,open=false]',
        '196:0': 'acacia_door[hinge=right,half=lower,powered=false,facing=east,open=false]',
        '196:1': 'acacia_door[hinge=right,half=lower,powered=false,facing=south,open=false]',
        '196:2': 'acacia_door[hinge=right,half=lower,powered=false,facing=west,open=false]',
        '196:3': 'acacia_door[hinge=right,half=lower,powered=false,facing=north,open=false]',
        '196:4': 'acacia_door[hinge=right,half=lower,powered=false,facing=east,open=true]',
        '196:5': 'acacia_door[hinge=right,half=lower,powered=false,facing=south,open=true]',
        '196:6': 'acacia_door[hinge=right,half=lower,powered=false,facing=west,open=true]',
        '196:7': 'acacia_door[hinge=right,half=lower,powered=false,facing=north,open=true]',
        '196:8': 'acacia_door[hinge=left,half=upper,powered=false,facing=east,open=false]',
        '196:9': 'acacia_door[hinge=right,half=upper,powered=false,facing=east,open=false]',
        '196:10': 'acacia_door[hinge=left,half=upper,powered=true,facing=east,open=false]',
        '196:11': 'acacia_door[hinge=right,half=upper,powered=true,facing=east,open=false]',
        '197:0': 'dark_oak_door[hinge=right,half=lower,powered=false,facing=east,open=false]',
        '197:1': 'dark_oak_door[hinge=right,half=lower,powered=false,facing=south,open=false]',
        '197:2': 'dark_oak_door[hinge=right,half=lower,powered=false,facing=west,open=false]',
        '197:3': 'dark_oak_door[hinge=right,half=lower,powered=false,facing=north,open=false]',
        '197:4': 'dark_oak_door[hinge=right,half=lower,powered=false,facing=east,open=true]',
        '197:5': 'dark_oak_door[hinge=right,half=lower,powered=false,facing=south,open=true]',
        '197:6': 'dark_oak_door[hinge=right,half=lower,powered=false,facing=west,open=true]',
        '197:7': 'dark_oak_door[hinge=right,half=lower,powered=false,facing=north,open=true]',
        '197:8': 'dark_oak_door[hinge=left,half=upper,powered=false,facing=east,open=false]',
        '197:9': 'dark_oak_door[hinge=right,half=upper,powered=false,facing=east,open=false]',
        '197:10': 'dark_oak_door[hinge=left,half=upper,powered=true,facing=east,open=false]',
        '197:11': 'dark_oak_door[hinge=right,half=upper,powered=true,facing=east,open=false]',
        '198:0': 'end_rod[facing=down]',
        '198:1': 'end_rod[facing=up]',
        '198:2': 'end_rod[facing=north]',
        '198:3': 'end_rod[facing=south]',
        '198:4': 'end_rod[facing=west]',
        '198:5': 'end_rod[facing=east]',
        '199:0': 'chorus_plant[east=false,south=false,north=false,west=false,up=false,down=false]',
        '200:0': 'chorus_flower[age=0]',
        '200:1': 'chorus_flower[age=1]',
        '200:2': 'chorus_flower[age=2]',
        '200:3': 'chorus_flower[age=3]',
        '200:4': 'chorus_flower[age=4]',
        '200:5': 'chorus_flower[age=5]',
        '201:0': 'purpur_block',
        '202:0': 'purpur_pillar[axis=y]',
        '202:4': 'purpur_pillar[axis=x]',
        '202:8': 'purpur_pillar[axis=z]',
        '203:0': 'purpur_stairs[half=bottom,shape=straight,facing=east]',
        '203:1': 'purpur_stairs[half=bottom,shape=straight,facing=west]',
        '203:2': 'purpur_stairs[half=bottom,shape=straight,facing=south]',
        '203:3': 'purpur_stairs[half=bottom,shape=straight,facing=north]',
        '203:4': 'purpur_stairs[half=top,shape=straight,facing=east]',
        '203:5': 'purpur_stairs[half=top,shape=straight,facing=west]',
        '203:6': 'purpur_stairs[half=top,shape=straight,facing=south]',
        '203:7': 'purpur_stairs[half=top,shape=straight,facing=north]',
        '204:0': 'purpur_slab[type=double]',
        '205:0': 'purpur_slab[type=bottom]',
        '205:8': 'purpur_slab[type=top]',
        '206:0': 'end_stone_bricks',
        '207:0': 'beetroots[age=0]',
        '207:1': 'beetroots[age=1]',
        '207:2': 'beetroots[age=2]',
        '207:3': 'beetroots[age=3]',
        '208:0': 'grass_path',
        '209:0': 'end_gateway',
        '210:0': 'repeating_command_block[conditional=false,facing=down]',
        '210:1': 'repeating_command_block[conditional=false,facing=up]',
        '210:2': 'repeating_command_block[conditional=false,facing=north]',
        '210:3': 'repeating_command_block[conditional=false,facing=south]',
        '210:4': 'repeating_command_block[conditional=false,facing=west]',
        '210:5': 'repeating_command_block[conditional=false,facing=east]',
        '210:8': 'repeating_command_block[conditional=true,facing=down]',
        '210:9': 'repeating_command_block[conditional=true,facing=up]',
        '210:10': 'repeating_command_block[conditional=true,facing=north]',
        '210:11': 'repeating_command_block[conditional=true,facing=south]',
        '210:12': 'repeating_command_block[conditional=true,facing=west]',
        '210:13': 'repeating_command_block[conditional=true,facing=east]',
        '211:0': 'chain_command_block[conditional=false,facing=down]',
        '211:1': 'chain_command_block[conditional=false,facing=up]',
        '211:2': 'chain_command_block[conditional=false,facing=north]',
        '211:3': 'chain_command_block[conditional=false,facing=south]',
        '211:4': 'chain_command_block[conditional=false,facing=west]',
        '211:5': 'chain_command_block[conditional=false,facing=east]',
        '211:8': 'chain_command_block[conditional=true,facing=down]',
        '211:9': 'chain_command_block[conditional=true,facing=up]',
        '211:10': 'chain_command_block[conditional=true,facing=north]',
        '211:11': 'chain_command_block[conditional=true,facing=south]',
        '211:12': 'chain_command_block[conditional=true,facing=west]',
        '211:13': 'chain_command_block[conditional=true,facing=east]',
        '212:0': 'frosted_ice[age=0]',
        '212:1': 'frosted_ice[age=1]',
        '212:2': 'frosted_ice[age=2]',
        '212:3': 'frosted_ice[age=3]',
        '213:0': 'magma_block',
        '214:0': 'nether_wart_block',
        '215:0': 'red_nether_bricks',
        '216:0': 'bone_block[axis=y]',
        '216:4': 'bone_block[axis=x]',
        '216:8': 'bone_block[axis=z]',
        '217:0': 'structure_void',
        '218:0': 'observer[powered=false,facing=down]',
        '218:1': 'observer[powered=false,facing=up]',
        '218:2': 'observer[powered=false,facing=north]',
        '218:3': 'observer[powered=false,facing=south]',
        '218:4': 'observer[powered=false,facing=west]',
        '218:5': 'observer[powered=false,facing=east]',
        '218:8': 'observer[powered=true,facing=down]',
        '218:9': 'observer[powered=true,facing=up]',
        '218:10': 'observer[powered=true,facing=north]',
        '218:11': 'observer[powered=true,facing=south]',
        '218:12': 'observer[powered=true,facing=west]',
        '218:13': 'observer[powered=true,facing=east]',
        '219:0': 'white_shulker_box[facing=down]',
        '219:1': 'white_shulker_box[facing=up]',
        '219:2': 'white_shulker_box[facing=north]',
        '219:3': 'white_shulker_box[facing=south]',
        '219:4': 'white_shulker_box[facing=west]',
        '219:5': 'white_shulker_box[facing=east]',
        '220:0': 'orange_shulker_box[facing=down]',
        '220:1': 'orange_shulker_box[facing=up]',
        '220:2': 'orange_shulker_box[facing=north]',
        '220:3': 'orange_shulker_box[facing=south]',
        '220:4': 'orange_shulker_box[facing=west]',
        '220:5': 'orange_shulker_box[facing=east]',
        '221:0': 'magenta_shulker_box[facing=down]',
        '221:1': 'magenta_shulker_box[facing=up]',
        '221:2': 'magenta_shulker_box[facing=north]',
        '221:3': 'magenta_shulker_box[facing=south]',
        '221:4': 'magenta_shulker_box[facing=west]',
        '221:5': 'magenta_shulker_box[facing=east]',
        '222:0': 'light_blue_shulker_box[facing=down]',
        '222:1': 'light_blue_shulker_box[facing=up]',
        '222:2': 'light_blue_shulker_box[facing=north]',
        '222:3': 'light_blue_shulker_box[facing=south]',
        '222:4': 'light_blue_shulker_box[facing=west]',
        '222:5': 'light_blue_shulker_box[facing=east]',
        '223:0': 'yellow_shulker_box[facing=down]',
        '223:1': 'yellow_shulker_box[facing=up]',
        '223:2': 'yellow_shulker_box[facing=north]',
        '223:3': 'yellow_shulker_box[facing=south]',
        '223:4': 'yellow_shulker_box[facing=west]',
        '223:5': 'yellow_shulker_box[facing=east]',
        '224:0': 'lime_shulker_box[facing=down]',
        '224:1': 'lime_shulker_box[facing=up]',
        '224:2': 'lime_shulker_box[facing=north]',
        '224:3': 'lime_shulker_box[facing=south]',
        '224:4': 'lime_shulker_box[facing=west]',
        '224:5': 'lime_shulker_box[facing=east]',
        '225:0': 'pink_shulker_box[facing=down]',
        '225:1': 'pink_shulker_box[facing=up]',
        '225:2': 'pink_shulker_box[facing=north]',
        '225:3': 'pink_shulker_box[facing=south]',
        '225:4': 'pink_shulker_box[facing=west]',
        '225:5': 'pink_shulker_box[facing=east]',
        '226:0': 'gray_shulker_box[facing=down]',
        '226:1': 'gray_shulker_box[facing=up]',
        '226:2': 'gray_shulker_box[facing=north]',
        '226:3': 'gray_shulker_box[facing=south]',
        '226:4': 'gray_shulker_box[facing=west]',
        '226:5': 'gray_shulker_box[facing=east]',
        '227:0': 'light_gray_shulker_box[facing=down]',
        '227:1': 'light_gray_shulker_box[facing=up]',
        '227:2': 'light_gray_shulker_box[facing=north]',
        '227:3': 'light_gray_shulker_box[facing=south]',
        '227:4': 'light_gray_shulker_box[facing=west]',
        '227:5': 'light_gray_shulker_box[facing=east]',
        '228:0': 'cyan_shulker_box[facing=down]',
        '228:1': 'cyan_shulker_box[facing=up]',
        '228:2': 'cyan_shulker_box[facing=north]',
        '228:3': 'cyan_shulker_box[facing=south]',
        '228:4': 'cyan_shulker_box[facing=west]',
        '228:5': 'cyan_shulker_box[facing=east]',
        '229:0': 'purple_shulker_box[facing=down]',
        '229:1': 'purple_shulker_box[facing=up]',
        '229:2': 'purple_shulker_box[facing=north]',
        '229:3': 'purple_shulker_box[facing=south]',
        '229:4': 'purple_shulker_box[facing=west]',
        '229:5': 'purple_shulker_box[facing=east]',
        '230:0': 'blue_shulker_box[facing=down]',
        '230:1': 'blue_shulker_box[facing=up]',
        '230:2': 'blue_shulker_box[facing=north]',
        '230:3': 'blue_shulker_box[facing=south]',
        '230:4': 'blue_shulker_box[facing=west]',
        '230:5': 'blue_shulker_box[facing=east]',
        '231:0': 'brown_shulker_box[facing=down]',
        '231:1': 'brown_shulker_box[facing=up]',
        '231:2': 'brown_shulker_box[facing=north]',
        '231:3': 'brown_shulker_box[facing=south]',
        '231:4': 'brown_shulker_box[facing=west]',
        '231:5': 'brown_shulker_box[facing=east]',
        '232:0': 'green_shulker_box[facing=down]',
        '232:1': 'green_shulker_box[facing=up]',
        '232:2': 'green_shulker_box[facing=north]',
        '232:3': 'green_shulker_box[facing=south]',
        '232:4': 'green_shulker_box[facing=west]',
        '232:5': 'green_shulker_box[facing=east]',
        '233:0': 'red_shulker_box[facing=down]',
        '233:1': 'red_shulker_box[facing=up]',
        '233:2': 'red_shulker_box[facing=north]',
        '233:3': 'red_shulker_box[facing=south]',
        '233:4': 'red_shulker_box[facing=west]',
        '233:5': 'red_shulker_box[facing=east]',
        '234:0': 'black_shulker_box[facing=down]',
        '234:1': 'black_shulker_box[facing=up]',
        '234:2': 'black_shulker_box[facing=north]',
        '234:3': 'black_shulker_box[facing=south]',
        '234:4': 'black_shulker_box[facing=west]',
        '234:5': 'black_shulker_box[facing=east]',
        '235:0': 'white_glazed_terracotta[facing=south]',
        '235:1': 'white_glazed_terracotta[facing=west]',
        '235:2': 'white_glazed_terracotta[facing=north]',
        '235:3': 'white_glazed_terracotta[facing=east]',
        '236:0': 'orange_glazed_terracotta[facing=south]',
        '236:1': 'orange_glazed_terracotta[facing=west]',
        '236:2': 'orange_glazed_terracotta[facing=north]',
        '236:3': 'orange_glazed_terracotta[facing=east]',
        '237:0': 'magenta_glazed_terracotta[facing=south]',
        '237:1': 'magenta_glazed_terracotta[facing=west]',
        '237:2': 'magenta_glazed_terracotta[facing=north]',
        '237:3': 'magenta_glazed_terracotta[facing=east]',
        '238:0': 'light_blue_glazed_terracotta[facing=south]',
        '238:1': 'light_blue_glazed_terracotta[facing=west]',
        '238:2': 'light_blue_glazed_terracotta[facing=north]',
        '238:3': 'light_blue_glazed_terracotta[facing=east]',
        '239:0': 'yellow_glazed_terracotta[facing=south]',
        '239:1': 'yellow_glazed_terracotta[facing=west]',
        '239:2': 'yellow_glazed_terracotta[facing=north]',
        '239:3': 'yellow_glazed_terracotta[facing=east]',
        '240:0': 'lime_glazed_terracotta[facing=south]',
        '240:1': 'lime_glazed_terracotta[facing=west]',
        '240:2': 'lime_glazed_terracotta[facing=north]',
        '240:3': 'lime_glazed_terracotta[facing=east]',
        '241:0': 'pink_glazed_terracotta[facing=south]',
        '241:1': 'pink_glazed_terracotta[facing=west]',
        '241:2': 'pink_glazed_terracotta[facing=north]',
        '241:3': 'pink_glazed_terracotta[facing=east]',
        '242:0': 'gray_glazed_terracotta[facing=south]',
        '242:1': 'gray_glazed_terracotta[facing=west]',
        '242:2': 'gray_glazed_terracotta[facing=north]',
        '242:3': 'gray_glazed_terracotta[facing=east]',
        '243:0': 'light_gray_glazed_terracotta[facing=south]',
        '243:1': 'light_gray_glazed_terracotta[facing=west]',
        '243:2': 'light_gray_glazed_terracotta[facing=north]',
        '243:3': 'light_gray_glazed_terracotta[facing=east]',
        '244:0': 'cyan_glazed_terracotta[facing=south]',
        '244:1': 'cyan_glazed_terracotta[facing=west]',
        '244:2': 'cyan_glazed_terracotta[facing=north]',
        '244:3': 'cyan_glazed_terracotta[facing=east]',
        '245:0': 'purple_glazed_terracotta[facing=south]',
        '245:1': 'purple_glazed_terracotta[facing=west]',
        '245:2': 'purple_glazed_terracotta[facing=north]',
        '245:3': 'purple_glazed_terracotta[facing=east]',
        '246:0': 'blue_glazed_terracotta[facing=south]',
        '246:1': 'blue_glazed_terracotta[facing=west]',
        '246:2': 'blue_glazed_terracotta[facing=north]',
        '246:3': 'blue_glazed_terracotta[facing=east]',
        '247:0': 'brown_glazed_terracotta[facing=south]',
        '247:1': 'brown_glazed_terracotta[facing=west]',
        '247:2': 'brown_glazed_terracotta[facing=north]',
        '247:3': 'brown_glazed_terracotta[facing=east]',
        '248:0': 'green_glazed_terracotta[facing=south]',
        '248:1': 'green_glazed_terracotta[facing=west]',
        '248:2': 'green_glazed_terracotta[facing=north]',
        '248:3': 'green_glazed_terracotta[facing=east]',
        '249:0': 'red_glazed_terracotta[facing=south]',
        '249:1': 'red_glazed_terracotta[facing=west]',
        '249:2': 'red_glazed_terracotta[facing=north]',
        '249:3': 'red_glazed_terracotta[facing=east]',
        '250:0': 'black_glazed_terracotta[facing=south]',
        '250:1': 'black_glazed_terracotta[facing=west]',
        '250:2': 'black_glazed_terracotta[facing=north]',
        '250:3': 'black_glazed_terracotta[facing=east]',
        '251:0': 'white_concrete',
        '251:1': 'orange_concrete',
        '251:2': 'magenta_concrete',
        '251:3': 'light_blue_concrete',
        '251:4': 'yellow_concrete',
        '251:5': 'lime_concrete',
        '251:6': 'pink_concrete',
        '251:7': 'gray_concrete',
        '251:8': 'light_gray_concrete',
        '251:9': 'cyan_concrete',
        '251:10': 'purple_concrete',
        '251:11': 'blue_concrete',
        '251:12': 'brown_concrete',
        '251:13': 'green_concrete',
        '251:14': 'red_concrete',
        '251:15': 'black_concrete',
        '252:0': 'white_concrete_powder',
        '252:1': 'orange_concrete_powder',
        '252:2': 'magenta_concrete_powder',
        '252:3': 'light_blue_concrete_powder',
        '252:4': 'yellow_concrete_powder',
        '252:5': 'lime_concrete_powder',
        '252:6': 'pink_concrete_powder',
        '252:7': 'gray_concrete_powder',
        '252:8': 'light_gray_concrete_powder',
        '252:9': 'cyan_concrete_powder',
        '252:10': 'purple_concrete_powder',
        '252:11': 'blue_concrete_powder',
        '252:12': 'brown_concrete_powder',
        '252:13': 'green_concrete_powder',
        '252:14': 'red_concrete_powder',
        '252:15': 'black_concrete_powder',
        '255:0': 'structure_block[mode=save]',
        '255:1': 'structure_block[mode=load]',
        '255:2': 'structure_block[mode=corner]',
        '255:3': 'structure_block[mode=data]',
    };
    const blockMap = new Map();
    for (const [id, state] of Object.entries(alphaMaterials)) {
        const [block, data] = id.split(':').map(s => Number(s));
        const dataMap = computeIfAbsent(blockMap, block, () => new Map());
        computeIfAbsent(dataMap, data, () => state);
    }
    function fromAlphaMaterial(block, data) {
        var _a, _b;
        const str = (_b = (_a = blockMap.get(block)) === null || _a === void 0 ? void 0 : _a.get(data)) !== null && _b !== void 0 ? _b : 'air';
        return BlockState.parse(str);
    }

    class MultiStructure {
        constructor(size, regions) {
            this.size = size;
            this.regions = regions;
        }
        getSize() {
            return this.size;
        }
        getBlock(pos) {
            for (const region of this.regions) {
                if (MultiStructure.posInRegion(pos, region)) {
                    const block = region.structure.getBlock(BlockPos.subtract(pos, region.pos));
                    if (block !== null) {
                        return block;
                    }
                }
            }
            return null;
        }
        getBlocks() {
            return this.regions.flatMap(r => {
                try {
                    return r.structure.getBlocks().map(b => (Object.assign({ pos: BlockPos.add(r.pos, b.pos), state: b.state }, b.nbt ? { nbt: b.nbt } : {})));
                }
                catch (e) {
                    if (e instanceof Error) {
                        console.log(r.structure['blocks']);
                        e.message = e.message.replace(' in structure ', ` in structure region "${r.name}" `);
                    }
                    throw e;
                }
            });
        }
        static posInRegion(pos, region) {
            const size = region.structure.getSize();
            return pos[0] >= region.pos[0] && pos[0] < region.pos[0] + size[0]
                && pos[1] >= region.pos[1] && pos[1] < region.pos[1] + size[1]
                && pos[2] >= region.pos[2] && pos[2] < region.pos[2] + size[2];
        }
    }

    function getTriple(tag) {
        return [tag.getNumber('x'), tag.getNumber('y'), tag.getNumber('z')];
    }
    function spongeToStructure(root) {
        var _a, _b;
        const width = root.getNumber('Width');
        const height = root.getNumber('Height');
        const length = root.getNumber('Length');
        const schemPalette = root.getCompound('Palette');
        const palette = [];
        for (const key of schemPalette.keys()) {
            const id = schemPalette.getNumber(key);
            palette[id] = BlockState.parse(key);
        }
        const blockData = root.getByteArray('BlockData').map(e => e.getAsNumber());
        const blockEntities = new Map();
        root.getList('BlockEntities', NbtType.Compound).forEach((tag) => {
            const pos = tag.getIntArray('Pos').toString();
            const copy = NbtCompound.fromJson(tag.toJson());
            copy.delete('Pos');
            blockEntities.set(pos, copy);
        });
        const blocks = [];
        let i = 0;
        for (let y = 0; y < height; y += 1) {
            for (let z = 0; z < length; z += 1) {
                for (let x = 0; x < width; x += 1) {
                    let id = (_a = blockData[i]) !== null && _a !== void 0 ? _a : 0;
                    i += 1;
                    if (id > 127) {
                        id += (((_b = blockData[i]) !== null && _b !== void 0 ? _b : 0) - 1) << 7;
                        i += 1;
                    }
                    const pos = new NbtIntArray([x, y, z]).toString();
                    blocks.push({
                        pos: [x, y, z],
                        state: id,
                        nbt: blockEntities.get(pos),
                    });
                }
            }
        }
        return new Structure([width, height, length], palette, blocks);
    }
    function litematicToStructure(root) {
        const enclosingSize = root.getCompound('Metadata').getCompound('EnclosingSize');
        const [width, height, length] = getTriple(enclosingSize);
        const regions = [];
        root.getCompound('Regions').forEach((name, region) => {
            if (!region.isCompound())
                return;
            const pos = getTriple(region.getCompound('Position'));
            const size = getTriple(region.getCompound('Size'));
            for (let i = 0; i < 3; i += 1) {
                if (size[i] < 0) {
                    pos[i] += size[i];
                    size[i] = -size[i];
                }
            }
            const volume = size[0] * size[1] * size[2];
            const palette = region.getList('BlockStatePalette', NbtType.Compound).map(BlockState.fromNbt);
            const blockStates = region.getLongArray('BlockStates');
            const tempDataview = new DataView(new Uint8Array(8).buffer);
            const statesData = blockStates.map(long => {
                tempDataview.setInt32(0, Number(long.getAsPair()[0]));
                tempDataview.setInt32(4, Number(long.getAsPair()[1]));
                return tempDataview.getBigUint64(0);
            });
            // litematica use at least 2 bits for palette indices (https://github.com/misode/vscode-nbt/issues/76)
            const bits = Math.max(2, Math.ceil(Math.log2(palette.length)));
            const bigBits = BigInt(bits);
            const big0 = BigInt(0);
            const big64 = BigInt(64);
            const bitMask = BigInt(Math.pow(2, bits) - 1);
            let state = 0;
            let data = statesData[state];
            let dataLength = big64;
            const arr = [];
            for (let j = 0; j < volume; j += 1) {
                if (dataLength < bits) {
                    state += 1;
                    let newData = statesData[state];
                    if (newData === undefined) {
                        console.error(`Out of bounds states access ${state}`);
                        newData = big0;
                    }
                    {
                        data = (newData << dataLength) | data;
                        dataLength += big64;
                    }
                }
                let paletteId = Number(data & bitMask);
                if (paletteId > palette.length - 1) {
                    console.error(`Invalid palette ID ${paletteId}`);
                    paletteId = 0;
                }
                arr.push(paletteId);
                data >>= bigBits;
                dataLength -= bigBits;
            }
            const blocks = [];
            const blockEntities = new Map();
            region.getList('TileEntities', NbtType.Compound).forEach((tag) => {
                const pos = getTriple(tag).toString();
                const copy = NbtCompound.fromJson(tag.toJson());
                copy.delete('x');
                copy.delete('y');
                copy.delete('z');
                blockEntities.set(pos, copy);
            });
            for (let x = 0; x < size[0]; x += 1) {
                for (let y = 0; y < size[1]; y += 1) {
                    for (let z = 0; z < size[2]; z += 1) {
                        const index = (y * size[0] * size[2]) + z * size[0] + x;
                        const pos = [x, y, z].toString();
                        blocks.push({
                            pos: [x, y, z],
                            state: arr[index],
                            nbt: blockEntities.get(pos),
                        });
                    }
                }
            }
            const structure = new Structure(size, palette, blocks);
            regions.push({ pos, structure, name });
        });
        const minPos = [Number.MAX_SAFE_INTEGER, Number.MAX_SAFE_INTEGER, Number.MAX_SAFE_INTEGER];
        for (const region of regions) {
            for (let i = 0; i < 3; i += 1) {
                minPos[i] = Math.min(minPos[i], region.pos[i]);
            }
        }
        for (const region of regions) {
            for (let i = 0; i < 3; i += 1) {
                region.pos[i] -= minPos[i];
            }
        }
        return new MultiStructure([width, height, length], regions);
    }
    function schematicToStructure(root) {
        const width = root.getNumber('Width');
        const height = root.getNumber('Height');
        const length = root.getNumber('Length');
        const blocksArray = root.getByteArray('Blocks').map(e => e.getAsNumber());
        const dataArray = root.getByteArray('Data').map(e => e.getAsNumber());
        const blockEntities = new Map();
        root.getList('TileEntities', NbtType.Compound).forEach((tag) => {
            const pos = getTriple(tag).toString();
            const copy = NbtCompound.fromJson(tag.toJson());
            copy.delete('x');
            copy.delete('y');
            copy.delete('z');
            blockEntities.set(pos, copy);
        });
        const structure = new Structure([width, height, length]);
        for (let x = 0; x < width; x += 1) {
            for (let y = 0; y < height; y += 1) {
                for (let z = 0; z < length; z += 1) {
                    const i = (y * width * length) + z * width + x;
                    const blockStata = fromAlphaMaterial(blocksArray[i], dataArray[i]);
                    const nbt = blockEntities.get([x, y, z].toString());
                    structure.addBlock([x, y, z], blockStata.getName(), blockStata.getProperties(), nbt);
                }
            }
        }
        return structure;
    }

    class TreeEditor {
        constructor(root, vscode, editHandler, readOnly) {
            this.root = root;
            this.vscode = vscode;
            this.editHandler = editHandler;
            this.readOnly = readOnly;
            this.onKey = (evt) => {
                const s = this.selected;
                if (evt.key === 'Delete' && s) {
                    this.removeTag(s.path, s.tag, s.el);
                }
                else if (evt.key === 'F2' && s) {
                    this.renameTag(s.path, s.tag, s.el);
                }
                else if (evt.key === 'Escape') {
                    if (this.editing === null) {
                        this.select(null);
                    }
                    else {
                        this.clearEditing();
                    }
                }
            };
            this.expanded = new Set();
            this.content = document.createElement('div');
            this.content.className = 'nbt-content';
            this.file = NbtFile.create();
            this.prefix = new NbtPath();
            this.pathToElement = { childs: {} };
            this.highlighted = null;
            this.selected = null;
            this.editing = null;
        }
        reveal() {
            this.root.append(this.content);
            if (this.selected) {
                this.select(this.selected);
            }
            document.addEventListener('keydown', this.onKey);
        }
        hide() {
            document.removeEventListener('keydown', this.onKey);
        }
        onInit(file, prefix) {
            if (prefix) {
                this.prefix = prefix;
            }
            this.file = file;
            this.expand(this.prefix);
            const rootKeys = [...this.file.root.keys()];
            if (rootKeys.length === 1) {
                this.expand(this.prefix.push(rootKeys[0]));
            }
            this.select(null);
            this.editing = null;
            this.redraw();
        }
        onUpdate(data) {
            this.onInit(data);
        }
        onSearch(query) {
            if (query === null) {
                const prevHighlight = this.highlighted;
                this.highlighted = null;
                this.hidePath(prevHighlight);
                return [];
            }
            const searchResults = searchNodes(this.file.root, query);
            return searchResults.map(path => ({
                path,
                show: () => this.showPath(path),
                replace: (query) => replaceNode(this.file.root, path, query),
            }));
        }
        async showPath(path) {
            var _a;
            if ((_a = this.highlighted) === null || _a === void 0 ? void 0 : _a.equals(path)) {
                return;
            }
            const redrawStart = path.pop().subPaths()
                .find(p => !this.expanded.has(p.toString()));
            const prevHighlight = this.highlighted;
            this.highlighted = path;
            if (redrawStart) {
                const tag = getNode(this.file.root, redrawStart);
                const el = this.getPathElement(redrawStart);
                if (el) {
                    await this.openBody(redrawStart, tag, el);
                }
            }
            this.hidePath(prevHighlight);
            const resultEl = this.getPathElement(path);
            if (resultEl) {
                resultEl.classList.add('highlighted');
                const bounds = resultEl.getBoundingClientRect();
                if (bounds.bottom > window.innerHeight || bounds.top < 0) {
                    resultEl.scrollIntoView({ block: 'center' });
                }
            }
        }
        hidePath(prevHighlight) {
            this.root.querySelectorAll('.nbt-tag.highlighted')
                .forEach(e => e.classList.remove('highlighted'));
            const pathToClose = prevHighlight === null || prevHighlight === void 0 ? void 0 : prevHighlight.subPaths().find(p => !this.isExpanded(p));
            if (pathToClose) {
                const el = this.getPathElement(pathToClose);
                if (el && el.classList.contains('collapse')) {
                    this.closeBody(pathToClose, el);
                }
            }
        }
        menu() {
            if (this.readOnly)
                return [];
            const actionButton = (action, fn) => {
                const el = document.createElement('div');
                el.className = `btn btn-${action}-tag disabled`;
                el.textContent = locale(`${action}Tag`);
                el.addEventListener('click', () => {
                    if (!this.selected)
                        return;
                    this.selected.el.scrollIntoView({ block: 'center' });
                    fn.bind(this)(this.selected.path, this.selected.tag, this.selected.el);
                });
                return el;
            };
            const editTag = actionButton('edit', this.clickTag);
            const removeTag = actionButton('remove', this.removeTag);
            const addTag = actionButton('add', this.addTag);
            const renameTag = actionButton('rename', this.renameTag);
            return [removeTag, editTag, addTag, renameTag];
        }
        redraw() {
            this.pathToElement = { childs: {} };
            const root = this.drawTag(this.prefix, this.file.root);
            this.content.innerHTML = '';
            this.content.append(root);
        }
        isExpanded(path) {
            var _a;
            const p = path.toString();
            return this.expanded.has(p) || ((_a = this.highlighted) === null || _a === void 0 ? void 0 : _a.pop().startsWith(path));
        }
        collapse(path) {
            const p = path.toString();
            this.expanded.delete(p);
        }
        expand(path) {
            path.subPaths().forEach(p => {
                this.expanded.add(p.toString());
            });
        }
        select(selected) {
            if (this.readOnly)
                return;
            this.selected = selected;
            this.root.querySelectorAll('.nbt-tag.selected').forEach(e => e.classList.remove('selected'));
            if (selected) {
                this.expand(selected.path.pop());
                this.root.querySelectorAll('.nbt-tag.highlighted').forEach(e => e.classList.remove('highlighted'));
                selected.el.classList.add('selected');
            }
            const btnEditTag = document.querySelector('.btn-edit-tag');
            btnEditTag === null || btnEditTag === void 0 ? void 0 : btnEditTag.classList.toggle('disabled', !selected || this.canExpand(selected.tag));
            const btnAddTag = document.querySelector('.btn-add-tag');
            btnAddTag === null || btnAddTag === void 0 ? void 0 : btnAddTag.classList.toggle('disabled', !selected || !this.canExpand(selected.tag));
            const parent = selected ? getNode(this.file.root, selected.path.pop()) : null;
            const btnRenameTag = document.querySelector('.btn-rename-tag');
            btnRenameTag === null || btnRenameTag === void 0 ? void 0 : btnRenameTag.classList.toggle('disabled', !(parent === null || parent === void 0 ? void 0 : parent.isCompound()));
            const btnRemoveTag = document.querySelector('.btn-remove-tag');
            btnRemoveTag === null || btnRemoveTag === void 0 ? void 0 : btnRemoveTag.classList.toggle('disabled', !this.selected || this.selected.path.length() === 0);
        }
        setPathElement(path, el) {
            let node = this.pathToElement;
            for (const e of path.arr) {
                if (!node.childs)
                    node.childs = {};
                if (!node.childs[e])
                    node.childs[e] = {};
                node = node.childs[e];
            }
            node.element = el;
        }
        getPathElement(path) {
            let node = this.pathToElement;
            for (const e of path.arr) {
                if (!node.childs || !node.childs[e])
                    return undefined;
                node = node.childs[e];
            }
            return node.element;
        }
        drawTag(path, tag) {
            var _a;
            const expanded = this.canExpand(tag) && this.isExpanded(path);
            const el = document.createElement('div');
            const head = document.createElement('div');
            this.setPathElement(path, head);
            head.classList.add('nbt-tag');
            if ((_a = this.highlighted) === null || _a === void 0 ? void 0 : _a.equals(path)) {
                head.classList.add('highlighted');
            }
            if (this.canExpand(tag)) {
                head.classList.add('collapse');
                head.append(this.drawCollapse(path, () => this.clickTag(path, tag, head)));
            }
            head.append(this.drawIcon(tag));
            if (typeof path.last() === 'string') {
                head.append(this.drawKey(`${path.last()}: `));
            }
            head.append(this.drawTagHeader(tag));
            head.addEventListener('click', () => {
                var _a;
                if (head === ((_a = this.selected) === null || _a === void 0 ? void 0 : _a.el))
                    return;
                this.clearEditing();
                this.select({ path, tag, el: head });
            });
            head.addEventListener('dblclick', () => {
                this.clickTag(path, tag, head);
            });
            el.append(head);
            const body = expanded
                ? this.drawTagBody(path, tag)
                : document.createElement('div');
            body.classList.add('nbt-body');
            el.append(body);
            return el;
        }
        canExpand(tag) {
            return TreeEditor.EXPANDABLE_TYPES.has(typeof tag === 'number' ? tag : tag.getId());
        }
        drawTagHeader(tag) {
            try {
                if (tag.isCompound()) {
                    return this.drawEntries(tag.size);
                }
                else if (tag.isList()) {
                    return this.drawEntries(tag.length);
                }
                else if (tag.isArray()) {
                    return this.drawEntries(tag.length);
                }
                else {
                    return this.drawPrimitiveTag(tag);
                }
            }
            catch (e) {
                this.vscode.postMessage({ type: 'error', body: e.message });
                return this.drawError(e.message);
            }
        }
        drawTagBody(path, tag) {
            try {
                switch (tag.getId()) {
                    case NbtType.Compound: return this.drawCompound(path, tag);
                    case NbtType.List: return this.drawList(path, tag);
                    case NbtType.ByteArray: return this.drawArray(path, tag);
                    case NbtType.IntArray: return this.drawArray(path, tag);
                    case NbtType.LongArray: return this.drawArray(path, tag);
                    default: return document.createElement('div');
                }
            }
            catch (e) {
                this.vscode.postMessage({ type: 'error', body: e.message });
                return this.drawError(e.message);
            }
        }
        drawError(message) {
            const el = document.createElement('span');
            el.classList.add('error');
            el.textContent = `Error "${message}"`;
            return el;
        }
        drawIcon(tag) {
            const el = document.createElement('span');
            el.setAttribute('data-icon', NbtType[tag.getId()]);
            return el;
        }
        drawKey(key) {
            const el = document.createElement('span');
            el.classList.add('nbt-key');
            el.textContent = key;
            return el;
        }
        drawCollapse(path, handler) {
            const el = document.createElement('span');
            el.classList.add('nbt-collapse');
            el.textContent = this.isExpanded(path) ? '-' : '+';
            el.addEventListener('click', () => handler());
            return el;
        }
        drawEntries(length) {
            const el = document.createElement('span');
            el.classList.add('nbt-entries');
            el.textContent = `${length} entr${length === 1 ? 'y' : 'ies'}`;
            return el;
        }
        drawCompound(path, tag) {
            const el = document.createElement('div');
            [...tag.keys()].sort().forEach(key => {
                const child = this.drawTag(path.push(key), tag.get(key));
                el.append(child);
            });
            return el;
        }
        drawList(path, tag) {
            const el = document.createElement('div');
            tag.forEach((v, i) => {
                const child = this.drawTag(path.push(i), v);
                el.append(child);
            });
            return el;
        }
        drawArray(path, tag) {
            if (!tag.isArray()) {
                throw new Error(`Trying to draw an array, but got a ${NbtType[tag.getId()]}`);
            }
            const el = document.createElement('div');
            tag.forEach((v, i) => {
                const child = this.drawTag(path.push(i), v);
                el.append(child);
            });
            return el;
        }
        drawPrimitiveTag(tag) {
            const el = document.createElement('span');
            el.classList.add('nbt-value');
            el.textContent = serializePrimitive(tag);
            return el;
        }
        clickTag(path, tag, el) {
            if (this.canExpand(tag)) {
                this.clickExpandableTag(path, tag, el);
            }
            else {
                this.clickPrimitiveTag(path, tag, el);
            }
        }
        clickExpandableTag(path, tag, el) {
            if (this.expanded.has(path.toString())) {
                this.collapse(path);
                this.closeBody(path, el);
            }
            else {
                this.expand(path);
                this.openBody(path, tag, el);
            }
        }
        closeBody(path, el) {
            el.nextElementSibling.innerHTML = '';
            el.querySelector('.nbt-collapse').textContent = '+';
        }
        async openBody(path, tag, el) {
            el.querySelector('.nbt-collapse').textContent = '-';
            const body = el.nextElementSibling;
            await new Promise((res) => setTimeout(res));
            body.innerHTML = '';
            body.append(this.drawTagBody(path, tag));
        }
        clickPrimitiveTag(path, tag, el) {
            var _a;
            if (this.readOnly)
                return;
            (_a = el.querySelector('span.nbt-value')) === null || _a === void 0 ? void 0 : _a.remove();
            const value = serializePrimitive(tag);
            const valueEl = document.createElement('input');
            el.append(valueEl);
            valueEl.classList.add('nbt-value');
            valueEl.value = value;
            valueEl.focus();
            valueEl.setSelectionRange(value.length, value.length);
            valueEl.scrollLeft = valueEl.scrollWidth;
            const confirmButton = document.createElement('button');
            el.append(confirmButton);
            confirmButton.classList.add('nbt-confirm');
            confirmButton.textContent = locale('confirm');
            const makeEdit = () => {
                const newTag = parsePrimitive(tag.getId(), valueEl.value).toJsonWithId();
                const oldTag = tag.toJsonWithId();
                if (JSON.stringify(oldTag) !== JSON.stringify(newTag)) {
                    this.editHandler({ type: 'set', path: path.arr, old: oldTag, new: newTag });
                }
            };
            confirmButton.addEventListener('click', makeEdit);
            valueEl.addEventListener('keyup', evt => {
                if (evt.key === 'Enter') {
                    makeEdit();
                }
            });
            this.setEditing(path, tag, el);
        }
        removeTag(path, tag, el) {
            if (this.readOnly)
                return;
            this.editHandler({ type: 'remove', path: path.arr, value: tag.toJsonWithId() });
        }
        addTag(path, tag, el) {
            if (this.readOnly)
                return;
            const body = el.nextElementSibling;
            const root = document.createElement('div');
            body.prepend(root);
            const nbtTag = document.createElement('div');
            nbtTag.classList.add('nbt-tag');
            root.append(nbtTag);
            const typeRoot = document.createElement('div');
            nbtTag.append(typeRoot);
            const keyInput = document.createElement('input');
            if (tag.isCompound()) {
                keyInput.classList.add('nbt-key');
                keyInput.placeholder = locale('name');
                nbtTag.append(keyInput);
            }
            const valueInput = document.createElement('input');
            valueInput.classList.add('nbt-value');
            valueInput.placeholder = locale('value');
            nbtTag.append(valueInput);
            const typeSelect = document.createElement('select');
            if (tag.isCompound() || (tag.isList() && tag.length === 0)) {
                typeRoot.classList.add('type-select');
                typeRoot.setAttribute('data-icon', 'Byte');
                typeRoot.append(typeSelect);
                TYPES.filter(t => t !== 'End').forEach(t => {
                    const option = document.createElement('option');
                    option.value = t;
                    option.textContent = t.charAt(0).toUpperCase() + t.slice(1).split(/(?=[A-Z])/).join(' ');
                    typeSelect.append(option);
                });
                const onChangeType = () => {
                    typeRoot.setAttribute('data-icon', typeSelect.value);
                    const typeSelectId = NbtType[typeSelect.value];
                    valueInput.classList.toggle('hidden', this.canExpand(typeSelectId));
                };
                typeSelect.focus();
                typeSelect.addEventListener('change', onChangeType);
                const hotKeys = {
                    c: NbtType.Compound,
                    l: NbtType.List,
                    s: NbtType.String,
                    b: NbtType.Byte,
                    t: NbtType.Short,
                    i: NbtType.Int,
                    g: NbtType.Long,
                    f: NbtType.Float,
                    d: NbtType.Double,
                };
                typeSelect.addEventListener('keydown', evt => {
                    var _a;
                    if (hotKeys[evt.key]) {
                        typeSelect.value = NbtType[hotKeys[evt.key]];
                        onChangeType();
                        evt.preventDefault();
                        (_a = nbtTag.querySelector('input')) === null || _a === void 0 ? void 0 : _a.focus();
                    }
                });
            }
            else if (tag.isListOrArray()) {
                const keyType = tag.getType();
                typeRoot.setAttribute('data-icon', NbtType[keyType]);
                valueInput.focus();
            }
            const confirmButton = document.createElement('button');
            nbtTag.append(confirmButton);
            confirmButton.classList.add('nbt-confirm');
            confirmButton.textContent = locale('confirm');
            const makeEdit = () => {
                const valueType = (tag.isCompound() || (tag.isList() && tag.length === 0))
                    ? NbtType[typeSelect.value]
                    : (tag.isListOrArray() ? tag.getType() : NbtType.End);
                const last = tag.isCompound() ? keyInput.value : 0;
                let newTag = NbtTag.create(valueType);
                if (!this.canExpand(valueType)) {
                    try {
                        newTag = parsePrimitive(valueType, valueInput.value);
                    }
                    catch (e) { }
                }
                const edit = { type: 'add', path: path.push(last).arr, value: newTag.toJsonWithId() };
                this.expand(path);
                this.editHandler(edit);
            };
            confirmButton.addEventListener('click', makeEdit);
            valueInput.addEventListener('keyup', evt => {
                if (evt.key === 'Enter') {
                    makeEdit();
                }
            });
            this.setEditing(null, tag, nbtTag);
        }
        renameTag(path, tag, el) {
            var _a;
            if (this.readOnly)
                return;
            (_a = el.querySelector('span.nbt-key')) === null || _a === void 0 ? void 0 : _a.remove();
            const valueEl = el.querySelector('.nbt-value, .nbt-entries');
            const key = path.last();
            const keyEl = document.createElement('input');
            el.insertBefore(keyEl, valueEl);
            keyEl.classList.add('nbt-value');
            keyEl.value = key;
            keyEl.focus();
            keyEl.setSelectionRange(key.length, key.length);
            keyEl.scrollLeft = keyEl.scrollWidth;
            const confirmButton = document.createElement('button');
            el.insertBefore(confirmButton, valueEl);
            confirmButton.classList.add('nbt-confirm');
            confirmButton.textContent = locale('confirm');
            const makeEdit = () => {
                const newKey = keyEl.value;
                if (key !== newKey) {
                    this.editHandler({ type: 'move', source: path.arr, path: path.pop().push(newKey).arr });
                }
                this.clearEditing();
            };
            confirmButton.addEventListener('click', makeEdit);
            keyEl.addEventListener('keyup', evt => {
                if (evt.key === 'Enter') {
                    makeEdit();
                }
            });
            this.setEditing(path, tag, el);
        }
        clearEditing() {
            var _a;
            if (this.editing && this.editing.el.parentElement) {
                if (this.editing.path === null) {
                    this.editing.el.parentElement.remove();
                }
                else {
                    const tag = this.drawTag(this.editing.path, this.editing.tag);
                    this.editing.el.parentElement.replaceWith(tag);
                    if (((_a = this.selected) === null || _a === void 0 ? void 0 : _a.el) === this.editing.el) {
                        this.selected.el = tag.firstElementChild;
                        this.selected.el.classList.add('selected');
                    }
                }
            }
            this.editing = null;
        }
        setEditing(path, tag, el) {
            this.clearEditing();
            this.editing = { path, tag, el };
        }
    }
    TreeEditor.EXPANDABLE_TYPES = new Set([NbtType.Compound, NbtType.List, NbtType.ByteArray, NbtType.IntArray, NbtType.LongArray]);

    function clamp(a, b, c) {
        return Math.max(b, Math.min(c, a));
    }
    function clampVec3(a, b, c) {
        a[0] = clamp(a[0], b[0], c[0]);
        a[1] = clamp(a[1], b[1], c[1]);
        a[2] = clamp(a[2], b[2], c[2]);
    }
    function negVec3(a) {
        return fromValues$1(-a[0], -a[1], -a[2]);
    }
    function getInt(el) {
        var _a;
        const value = parseInt((_a = el) === null || _a === void 0 ? void 0 : _a.value);
        return isNaN(value) ? undefined : value;
    }
    class StructureEditor {
        constructor(root, vscode, editHandler, readOnly) {
            this.root = root;
            this.vscode = vscode;
            this.editHandler = editHandler;
            this.readOnly = readOnly;
            this.renderRequested = false;
            this.movement = [0, 0, 0, 0, 0, 0];
            this.movementKeys = ['KeyW', 'KeyA', 'KeyS', 'KeyD', 'Space', 'ShiftLeft'];
            this.onKeyDown = (evt) => {
                const index = this.movementKeys.indexOf(evt.code);
                if (index !== -1) {
                    this.movement[index] = 1;
                    this.render();
                }
            };
            this.onKeyUp = (evt) => {
                const index = this.movementKeys.indexOf(evt.code);
                if (index !== -1) {
                    this.movement[index] = 0;
                }
            };
            const assets = JSON.parse(stringifiedAssets);
            const uvMapping = JSON.parse(stringifiedUvmapping);
            const blocks = JSON.parse(stringifiedBlocks);
            const img = document.querySelector('.texture-atlas');
            this.resources = new ResourceManager(blocks, Object.assign(Object.assign({}, assets), { textures: uvMapping }), img);
            this.canvas = document.createElement('canvas');
            this.canvas.className = 'structure-3d';
            const gl = this.canvas.getContext('webgl', { preserveDrawingBuffer: true });
            this.structure = new Structure([0, 0, 0]);
            this.renderer = new StructureRenderer(gl, this.structure, this.resources);
            this.canvas2 = document.createElement('canvas');
            this.canvas2.className = 'structure-3d click-detection';
            this.gl2 = this.canvas2.getContext('webgl');
            this.renderer2 = new StructureRenderer(this.gl2, this.structure, this.resources);
            this.warning = document.createElement('div');
            this.warning.className = 'nbt-warning';
            const warningMsg = document.createElement('span');
            warningMsg.textContent = 'Trying to render a very large structure';
            this.warning.append(warningMsg);
            const warningButton = document.createElement('div');
            warningButton.className = 'btn active';
            warningButton.textContent = 'Continue';
            warningButton.addEventListener('click', () => {
                this.warning.classList.remove('active');
                if (this._cameraDebugEl) {
                    this._cameraDebugEl = null;
                }
                this.root.innerHTML = '<div class="spinner"></div>';
                setTimeout(() => {
                    this.buildStructure();
                    this.reveal();
                    this.render();
                });
            });
            this.warning.append(warningButton);
            this.cPos = create$1();
            this.cRot = fromValues(0.4, 0.6);
            this.cDist = 10;
            this.cFovDeg = typeof window !== 'undefined' && typeof window.__lbaCameraFov === 'number'
                ? Math.max(0, Math.min(110, window.__lbaCameraFov | 0))
                : 70;
            this.gridActive = true;
            this.invisibleBlocksActive = false;
            this.selectedBlock = null;
            let dragTime;
            let dragPos = null;
            let dragButton;
            this.canvas.addEventListener('mousedown', evt => {
                dragTime = Date.now();
                dragPos = [evt.clientX, evt.clientY];
                dragButton = evt.button;
            });
            this.canvas.addEventListener('mousemove', evt => {
                if (dragPos) {
                    const dx = (evt.clientX - dragPos[0]) / 100;
                    const dy = (evt.clientY - dragPos[1]) / 100;
                    if (dragButton === 0) {
                        add(this.cRot, this.cRot, [dx, dy]);
                        this.cRot[0] = this.cRot[0] % (Math.PI * 2);
                        this.cRot[1] = clamp(this.cRot[1], -Math.PI / 2, Math.PI / 2);
                    }
                    else if (dragButton === 2 || dragButton === 1) {
                        rotateY(this.cPos, this.cPos, [0, 0, 0], this.cRot[0]);
                        rotateX(this.cPos, this.cPos, [0, 0, 0], this.cRot[1]);
                        const d = fromValues$1(dx, -dy, 0);
                        scale(d, d, 0.25 * this.cDist);
                        add$1(this.cPos, this.cPos, d);
                        rotateX(this.cPos, this.cPos, [0, 0, 0], -this.cRot[1]);
                        rotateY(this.cPos, this.cPos, [0, 0, 0], -this.cRot[0]);
                        clampVec3(this.cPos, negVec3(this.structure.getSize()), [0, 0, 0]);
                    }
                    else {
                        return;
                    }
                    dragPos = [evt.clientX, evt.clientY];
                    this.render();
                }
            });
            this.canvas.addEventListener('mouseup', evt => {
                dragPos = null;
                if (Date.now() - dragTime < 200) {
                    if (dragButton === 0) {
                        this.selectBlock(evt.clientX, evt.clientY);
                    }
                }
            });
            this.canvas.addEventListener('wheel', evt => {
                this.cDist += evt.deltaY / 100;
                this.cDist = Math.max(1, Math.min(100, this.cDist));
                this.render();
            });
            window.addEventListener('resize', () => {
                if (this.resize())
                    this.render();
            });
            this._cameraDebugEnabled = false;
            this._cameraDebugEl = null;
            this._renderFollowUp = false;
            this._lbaFovRafId = null;
            /** 为完整导出时禁止将画布尺寸同步为 CSS 视口（否则会裁切）。 */
            this._lbaSkipCanvasResizeSync = false;
            this.render();
        }
        _lbaStartFovHostSync() {
            this._lbaStopFovHostSync();
            const step = () => {
                this._lbaFovRafId = requestAnimationFrame(step);
                if (typeof window === 'undefined' || typeof window.__lbaCameraFov !== 'number') {
                    return;
                }
                const nv = Math.max(0, Math.min(110, window.__lbaCameraFov | 0));
                if (nv !== this.cFovDeg) {
                    this.cFovDeg = nv;
                    this.render();
                }
            };
            this._lbaFovRafId = requestAnimationFrame(step);
        }
        _lbaStopFovHostSync() {
            if (this._lbaFovRafId != null) {
                cancelAnimationFrame(this._lbaFovRafId);
                this._lbaFovRafId = null;
            }
        }
        render() {
            if (this.renderRequested) {
                /* FOV 滑块等外部高频调用会在同一帧内多次触发 render；补排队一帧以使用最新的 cFovDeg。 */
                this._renderFollowUp = true;
                return;
            }
            const requestTime = performance.now();
            this.renderRequested = true;
            requestAnimationFrame((time) => {
                const delta = Math.max(0, time - requestTime);
                this.resize();
                const gl = this.renderer.gl;
                gl.bindFramebuffer(gl.FRAMEBUFFER, null);
                gl.clearColor(30 / 255, 30 / 255, 30 / 255, 1);
                gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
                if (this.movement.some(m => m)) {
                    rotateY(this.cPos, this.cPos, [0, 0, 0], this.cRot[0]);
                    const [w, a, s, d, space, shift] = this.movement;
                    const move = fromValues$1(a - d, shift - space, w - s);
                    scaleAndAdd(this.cPos, this.cPos, move, delta * 0.02);
                    rotateY(this.cPos, this.cPos, [0, 0, 0], -this.cRot[0]);
                    this.render();
                }
                const viewMatrix = this.getViewMatrix();
                if (this.gridActive) {
                    this.renderer.drawGrid(viewMatrix);
                }
                if (this.invisibleBlocksActive) {
                    this.renderer.drawInvisibleBlocks(viewMatrix);
                }
                this.renderer.drawStructure(viewMatrix);
                if (this.selectedBlock) {
                    this.renderer.drawOutline(viewMatrix, this.selectedBlock);
                }
                if (this._cameraDebugEnabled) {
                    this._updateCameraDebugOverlay();
                }
                this.renderRequested = false;
                if (this._renderFollowUp) {
                    this._renderFollowUp = false;
                    this.render();
                }
            });
        }
        resize() {
            const orthoH = Math.max(this.cDist * 0.38, 2.5);
            this.renderer2.lbaFovDeg = this.cFovDeg;
            this.renderer2.lbaOrthoHalfHeight = orthoH;
            this.renderer.lbaFovDeg = this.cFovDeg;
            this.renderer.lbaOrthoHalfHeight = orthoH;
            let changed = false;
            const displayWidth2 = this.canvas2.clientWidth;
            const displayHeight2 = this.canvas2.clientHeight;
            if (this.canvas2.width !== displayWidth2 || this.canvas2.height !== displayHeight2) {
                this.canvas2.width = displayWidth2;
                this.canvas2.height = displayHeight2;
                changed = true;
            }
            if (!this._lbaSkipCanvasResizeSync) {
                const displayWidth = this.canvas.clientWidth;
                const displayHeight = this.canvas.clientHeight;
                if (this.canvas.width !== displayWidth || this.canvas.height !== displayHeight) {
                    this.canvas.width = displayWidth;
                    this.canvas.height = displayHeight;
                    changed = true;
                }
            }
            if (this.canvas2.width > 0 && this.canvas2.height > 0) {
                this.renderer2.setViewport(0, 0, this.canvas2.width, this.canvas2.height);
            }
            if (this.canvas.width > 0 && this.canvas.height > 0) {
                this.renderer.setViewport(0, 0, this.canvas.width, this.canvas.height);
            }
            return changed;
        }
        reveal() {
            if (typeof window !== 'undefined' && typeof window.__lbaCameraFov === 'number') {
                this.cFovDeg = Math.max(0, Math.min(110, window.__lbaCameraFov | 0));
            }
            this.root.append(this.warning);
            this.root.append(this.canvas);
            this.root.append(this.canvas2);
            this.showSidePanel();
            document.addEventListener('keydown', this.onKeyDown);
            document.addEventListener('keyup', this.onKeyUp);
            if (typeof window !== 'undefined' && window.__lbaNbtCameraDebug) {
                this.setCameraDebug(true);
            }
            this._lbaStartFovHostSync();
        }
        hide() {
            this._lbaStopFovHostSync();
            document.removeEventListener('keydown', this.onKeyDown);
            document.removeEventListener('keyup', this.onKeyUp);
        }
        setCameraDebug(enabled) {
            this._cameraDebugEnabled = !!enabled;
            if (!this._cameraDebugEnabled) {
                if (this._cameraDebugEl) {
                    this._cameraDebugEl.remove();
                    this._cameraDebugEl = null;
                }
                return;
            }
            if (this._cameraDebugEl && (!this._cameraDebugEl.isConnected || !this.root.contains(this._cameraDebugEl))) {
                this._cameraDebugEl = null;
            }
            if (!this._cameraDebugEl && this.root) {
                const el = document.createElement('pre');
                el.className = 'lba-nbt-camera-debug';
                el.style.cssText = 'position:absolute;bottom:8px;right:8px;top:auto;left:auto;margin:0;padding:8px 10px;font:12px/1.35 ui-monospace,Consolas,monospace;white-space:pre;z-index:50;pointer-events:none;background:rgba(0,0,0,0.72);color:#e0e0e0;border-radius:4px;border:1px solid rgba(255,255,255,0.12);max-width:min(420px,90vw);text-align:left;';
                this._cameraDebugEl = el;
                const rs = this.root.style.position;
                if (!rs || rs === 'static') {
                    this.root.style.position = 'relative';
                }
                this.root.appendChild(el);
            }
            this._updateCameraDebugOverlay();
        }
        _updateCameraDebugOverlay() {
            if (!this._cameraDebugEnabled || !this._cameraDebugEl) {
                return;
            }
            if (!this._cameraDebugEl.isConnected || !this.root.contains(this._cameraDebugEl)) {
                this._cameraDebugEl = null;
                if (typeof window !== 'undefined' && window.__lbaNbtCameraDebug) {
                    this.setCameraDebug(true);
                }
                return;
            }
            const yaw = this.cRot[0];
            const pitch = this.cRot[1];
            const deg = (r) => r * 180 / Math.PI;
            const fmt = (n) => Math.round(n * 1000) / 1000;
            const sz = this.structure ? this.structure.getSize() : [0, 0, 0];
            const fovLine = this.cFovDeg <= 0
                ? 'FOV: 0°（正交投影）'
                : `FOV: ${Math.round(this.cFovDeg)}°（透视）`;
            this._cameraDebugEl.textContent = [
                'NBT 3D 相机（StructureEditor）',
                fovLine,
                `cPos（视图矩阵平移项）: (${fmt(this.cPos[0])}, ${fmt(this.cPos[1])}, ${fmt(this.cPos[2])})`,
                `cRot[0] yaw（绕 Y，弧度）: ${fmt(yaw)}  →  ${fmt(deg(yaw)).toFixed(2)}°`,
                `cRot[1] pitch（绕 X，弧度）: ${fmt(pitch)}  →  ${fmt(deg(pitch)).toFixed(2)}°`,
                `cDist（沿 -Z 后退距离）: ${fmt(this.cDist)}`,
                `结构尺寸 getSize(): (${sz[0]}, ${sz[1]}, ${sz[2]})`,
            ].join('\n');
        }
        onInit(file) {
            this.updateStructure(file);
            copy(this.cPos, this.structure.getSize());
            scale(this.cPos, this.cPos, -0.5);
            this.cDist = dist([0, 0, 0], this.cPos) * 1.5;
            this.render();
        }
        onUpdate(file, edit) {
            this.updateStructure(file);
            this.showSidePanel();
            this.render();
        }
        updateStructure(file) {
            this.file = file;
            this.structure = this.loadStructure();
            const isLarge = this.isLarge();
            const toggle = document.querySelector('.invisible-blocks-toggle');
            toggle === null || toggle === void 0 ? void 0 : toggle.classList.toggle('unavailable', isLarge);
            toggle === null || toggle === void 0 ? void 0 : toggle.setAttribute('title', isLarge ? locale('invisibleBlocksUnavailable') : '');
            if (isLarge) {
                this.warning.classList.add('active');
                return;
            }
            this.buildStructure();
        }
        isLarge() {
            const [x, y, z] = this.structure.getSize();
            const defaultThreshold = 48 * 48 * 48;
            const configuredThreshold = typeof window !== 'undefined' && typeof window.__lbaLargeStructureThreshold === 'number'
                ? Math.floor(window.__lbaLargeStructureThreshold)
                : defaultThreshold;
            const threshold = Math.max(1, configuredThreshold);
            return x * y * z > threshold;
        }
        loadStructure() {
            var _a, _b, _c;
            if (((_a = this.file.root.get('BlockData')) === null || _a === void 0 ? void 0 : _a.isByteArray()) && this.file.root.hasCompound('Palette')) {
                return spongeToStructure(this.file.root);
            }
            if (this.file.root.hasCompound('Regions')) {
                return litematicToStructure(this.file.root);
            }
            if (((_b = this.file.root.get('Blocks')) === null || _b === void 0 ? void 0 : _b.isByteArray()) && ((_c = this.file.root.get('Data')) === null || _c === void 0 ? void 0 : _c.isByteArray())) {
                return schematicToStructure(this.file.root);
            }
            return Structure.fromNbt(this.file.root);
        }
        buildStructure() {
            const isLarge = this.isLarge();
            this.renderer.useInvisibleBlocks = !isLarge;
            this.renderer2.useInvisibleBlocks = !isLarge;
            this.renderer.setStructure(this.structure);
            this.renderer2.setStructure(this.structure);
        }
        menu() {
            const gridToggle = document.createElement('div');
            gridToggle.classList.add('btn');
            gridToggle.textContent = locale('grid');
            gridToggle.classList.toggle('active', this.gridActive);
            gridToggle.addEventListener('click', () => {
                this.gridActive = !this.gridActive;
                gridToggle.classList.toggle('active', this.gridActive);
                this.render();
            });
            const invisibleBlocksToggle = document.createElement('div');
            invisibleBlocksToggle.classList.add('btn', 'invisible-blocks-toggle');
            invisibleBlocksToggle.textContent = locale('invisibleBlocks');
            invisibleBlocksToggle.classList.toggle('active', this.invisibleBlocksActive);
            invisibleBlocksToggle.addEventListener('click', () => {
                if (!this.renderer.useInvisibleBlocks)
                    return;
                this.invisibleBlocksActive = !this.invisibleBlocksActive;
                invisibleBlocksToggle.classList.toggle('active', this.invisibleBlocksActive);
                this.render();
            });
            return [gridToggle, invisibleBlocksToggle];
        }
        getViewMatrix() {
            const viewMatrix = create$2();
            translate(viewMatrix, viewMatrix, [0, 0, -this.cDist]);
            rotateX$1(viewMatrix, viewMatrix, this.cRot[1]);
            rotateY$1(viewMatrix, viewMatrix, this.cRot[0]);
            translate(viewMatrix, viewMatrix, this.cPos);
            return viewMatrix;
        }
        _lbaExportPngDataUrlFull(editorIsChunk) {
            /* 不调用 resize()、不改 canvas 尺寸（避免 WebGL 重置）。仅临时调整相机：结构中心 + 估算 cDist，使 AABB 在当前 FOV/正交下尽量入镜；保留 cRot。 */
            const saveDist = this.cDist;
            const savePos = create$1();
            copy(savePos, this.cPos);
            const saveFov = this.cFovDeg;
            try {
                const W = this.canvas.width;
                const H = this.canvas.height;
                if (W < 2 || H < 2) {
                    return '';
                }
                const sz = this.structure.getSize();
                const sx = Math.max(Number(sz[0]) || 0, 1);
                const sy = Math.max(Number(sz[1]) || 0, 1);
                const sz2 = Math.max(Number(sz[2]) || 0, 1);
                if (editorIsChunk) {
                    copy(this.cPos, sz);
                    mul(this.cPos, this.cPos, [-0.5, -1, -0.5]);
                    add$1(this.cPos, this.cPos, [0, 16, 0]);
                }
                else {
                    this.cPos[0] = -0.5 * sx;
                    this.cPos[1] = -0.5 * sy;
                    this.cPos[2] = -0.5 * sz2;
                }
                if (typeof window !== 'undefined' && typeof window.__lbaCameraFov === 'number') {
                    this.cFovDeg = Math.max(0, Math.min(110, window.__lbaCameraFov | 0));
                }
                const diag = Math.hypot(sx, sy, sz2);
                const fovDegEff = typeof this.cFovDeg === 'number' ? this.cFovDeg : 70;
                const _ep = (typeof window !== 'undefined' && window.__lbaExportFullParams && typeof window.__lbaExportFullParams === 'object')
                    ? window.__lbaExportFullParams : {};
                const _num = (v, d) => (typeof v === 'number' && Number.isFinite(v) ? v : d);
                const margin = Math.max(1.001, _num(_ep.margin, 1.22));
                const perspMinDist = Math.max(0.5, _num(_ep.perspectiveMinDist, 6));
                const perspDiagExtra = Math.max(0, _num(_ep.perspectiveDiagExtra, 0.12));
                const orthoNeedHalfPadding = Math.max(0, _num(_ep.orthographicNeedHalfPadding, 1));
                const orthoHeightScale = Math.max(0.05, _num(_ep.orthographicHeightScale, 0.38));
                const orthoDiagExtra = Math.max(0, _num(_ep.orthographicDiagExtra, 0.15));
                const orthoMinDist = Math.max(0.5, _num(_ep.orthographicMinDist, 12));
                const orthoHalfHeightMin = Math.max(0.1, _num(_ep.orthographicHalfHeightMin, 2.5));
                const r = diag * 0.5 * margin;
                if (fovDegEff <= 0) {
                    const needHalfH = r + orthoNeedHalfPadding;
                    this.cDist = Math.max(orthoMinDist, (needHalfH / orthoHeightScale) + diag * orthoDiagExtra);
                }
                else {
                    const aspect = Math.max(W / H, 1e-6);
                    const fovRad = Math.max(1, Math.min(110, fovDegEff)) * Math.PI / 180;
                    const tanHalfY = Math.tan(fovRad * 0.5);
                    const tanHalfX = tanHalfY * aspect;
                    const dVert = r / tanHalfY;
                    const dHorz = r / tanHalfX;
                    this.cDist = Math.max(perspMinDist, Math.max(dVert, dHorz) + diag * perspDiagExtra);
                }
                const orthoH = Math.max(this.cDist * orthoHeightScale, orthoHalfHeightMin);
                this.renderer.lbaFovDeg = this.cFovDeg;
                this.renderer.lbaOrthoHalfHeight = orthoH;
                this.renderer.setViewport(0, 0, W, H);
                const gl = this.renderer.gl;
                gl.bindFramebuffer(gl.FRAMEBUFFER, null);
                gl.viewport(0, 0, W, H);
                gl.clearColor(30 / 255, 30 / 255, 30 / 255, 1);
                gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
                const viewMatrix = this.getViewMatrix();
                if (this.gridActive) {
                    this.renderer.drawGrid(viewMatrix);
                }
                if (this.invisibleBlocksActive) {
                    this.renderer.drawInvisibleBlocks(viewMatrix);
                }
                this.renderer.drawStructure(viewMatrix);
                if (this.selectedBlock) {
                    this.renderer.drawOutline(viewMatrix, this.selectedBlock);
                }
                if (typeof gl.finish === 'function') {
                    gl.finish();
                }
                return this.canvas.toDataURL('image/png');
            }
            finally {
                this.cDist = saveDist;
                copy(this.cPos, savePos);
                this.cFovDeg = saveFov;
                this.render();
            }
        }
        selectBlock(x, y) {
            const viewMatrix = this.getViewMatrix();
            const gl2 = this.gl2;
            gl2.bindFramebuffer(gl2.FRAMEBUFFER, null);
            this.renderer2.setViewport(0, 0, this.canvas2.width, this.canvas2.height);
            gl2.clearColor(0, 0, 0, 0);
            gl2.clear(gl2.COLOR_BUFFER_BIT | gl2.DEPTH_BUFFER_BIT);
            this.renderer2.drawColoredStructure(viewMatrix);
            const color = new Uint8Array(4);
            this.gl2.readPixels(x, this.canvas2.height - y, 1, 1, this.gl2.RGBA, this.gl2.UNSIGNED_BYTE, color);
            const oldSelectedBlock = this.selectedBlock ? [...this.selectedBlock] : null;
            if (color[3] === 255) {
                this.selectedBlock = [color[0], color[1], color[2]];
            }
            else {
                this.selectedBlock = null;
            }
            if (JSON.stringify(oldSelectedBlock) !== JSON.stringify(this.selectedBlock)) {
                this.showSidePanel();
                this.render();
            }
        }
        showSidePanel() {
            var _a;
            (_a = this.root.querySelector('.side-panel')) === null || _a === void 0 ? void 0 : _a.remove();
            const block = this.selectedBlock ? this.structure.getBlock(this.selectedBlock) : null;
            const readOnly = this.readOnly || !this.file.root.hasList('size', NbtType.Int, 3);
            const sidePanel = document.createElement('div');
            sidePanel.classList.add('side-panel');
            this.root.append(sidePanel);
            if (block) {
                const properties = block.state.getProperties();
                sidePanel.innerHTML = `
				<div class="block-name">${block.state.getName()}</div>
				<div class="block-pos">${block.pos.join(' ')}</div>
				${Object.keys(properties).length === 0 ? '' : `
					<div class="block-props">
					${Object.entries(properties).map(([k, v]) => `
						<span class="prop-key">${k}</span>
						<span class="prop-value">${v}</span>
					`).join('')}
					</div>
				`}
			`;
                if (block.nbt) {
                    const nbtTree = document.createElement('div');
                    sidePanel.append(nbtTree);
                    const blockIndex = this.file.root.getList('blocks', NbtType.Compound).getItems()
                        .findIndex(t => BlockPos.equals(BlockPos.fromNbt(t.getList('pos')), block.pos));
                    const tree = new TreeEditor(nbtTree, this.vscode, edit => {
                        this.editHandler(mapEdit(edit, e => {
                            return Object.assign(Object.assign({}, e), { path: ['blocks', blockIndex, 'nbt', ...e.path] });
                        }));
                    }, readOnly);
                    tree.onInit(new NbtFile('', block.nbt, 'none', this.file.littleEndian, undefined));
                    tree.reveal();
                }
            }
            else {
                sidePanel.innerHTML = `
				<div class="structure-size">
					<label>Size</label>
					<input type="number" ${readOnly ? 'readonly' : ''}>
					<input type="number" ${readOnly ? 'readonly' : ''}>
					<input type="number" ${readOnly ? 'readonly' : ''}>
				</div>
			`;
                sidePanel.querySelectorAll('.structure-size input').forEach((el, i) => {
                    const original = this.structure.getSize()[i];
                    el.value = original.toString();
                    if (readOnly)
                        return;
                    el.addEventListener('change', () => {
                        this.editHandler({
                            type: 'set',
                            path: ['size', i],
                            old: new NbtInt(original).toJsonWithId(),
                            new: new NbtInt(parseInt(el.value)).toJsonWithId(),
                        });
                    });
                });
            }
        }
    }

    const VERSION_20w17a = 2529;
    const VERSION_21w43a = 2844;
    class ChunkEditor extends StructureEditor {
        onInit(file) {
            this.updateStructure(file);
            copy(this.cPos, this.structure.getSize());
            mul(this.cPos, this.cPos, [-0.5, -1, -0.5]);
            add$1(this.cPos, this.cPos, [0, 16, 0]);
            this.cDist = 25;
            this.showSidePanel();
            this.render();
        }
        loadStructure() {
            this.gridActive = false;
            const dataVersion = this.file.root.getNumber('DataVersion');
            const N = dataVersion >= VERSION_21w43a;
            const stretches = dataVersion < VERSION_20w17a;
            const sections = N
                ? this.file.root.getList('sections', NbtType.Compound)
                : this.file.root.getCompound('Level').getList('Sections', NbtType.Compound);
            const filledSections = sections.filter(section => {
                const palette = N
                    ? section.getCompound('block_states').getList('palette', NbtType.Compound)
                    : section.has('Palette') && section.getList('Palette', NbtType.Compound);
                return palette &&
                    palette.filter(state => state.getString('Name') !== 'minecraft:air')
                        .length > 0;
            });
            if (filledSections.length === 0) {
                throw new Error('Empty chunk');
            }
            const minY = 16 * Math.min(...filledSections.map(s => s.getNumber('Y')));
            const maxY = 16 * Math.max(...filledSections.map(s => s.getNumber('Y')));
            const K_palette = N ? 'palette' : 'Palette';
            const K_data = N ? 'data' : 'BlockStates';
            const structure = new Structure([16, maxY - minY + 16, 16]);
            for (const section of filledSections) {
                const states = N ? section.getCompound('block_states') : section;
                if (!states.has(K_palette) || !states.has(K_data)) {
                    continue;
                }
                const yOffset = section.getNumber('Y') * 16 - minY;
                const palette = states.getList(K_palette, NbtType.Compound);
                const blockStates = states.getLongArray(K_data);
                const tempDataview = new DataView(new Uint8Array(8).buffer);
                const statesData = blockStates.map(long => {
                    tempDataview.setInt32(0, Number(long.getAsPair()[0]));
                    tempDataview.setInt32(4, Number(long.getAsPair()[1]));
                    return tempDataview.getBigUint64(0);
                });
                const bits = Math.max(4, Math.ceil(Math.log2(palette.length)));
                const bigBits = BigInt(bits);
                const big64 = BigInt(64);
                const bitMask = BigInt(Math.pow(2, bits) - 1);
                let state = 0;
                let data = statesData[state];
                let dataLength = big64;
                for (let j = 0; j < 4096; j += 1) {
                    if (dataLength < bits) {
                        state += 1;
                        const newData = statesData[state];
                        if (stretches) {
                            data = (newData << dataLength) | data;
                            dataLength += big64;
                        }
                        else {
                            data = newData;
                            dataLength = big64;
                        }
                    }
                    const paletteId = data & bitMask;
                    const blockState = palette.get(Number(paletteId));
                    if (blockState) {
                        const pos = [j & 0xF, yOffset + (j >> 8), (j >> 4) & 0xF];
                        const block = BlockState.fromNbt(blockState);
                        structure.addBlock(pos, block.getName(), block.getProperties());
                    }
                    data >>= bigBits;
                    dataLength -= bigBits;
                }
            }
            console.log(structure);
            return structure;
        }
        menu() {
            return [];
        }
        showSidePanel() {
            var _a;
            (_a = this.root.querySelector('.side-panel')) === null || _a === void 0 ? void 0 : _a.remove();
            const block = this.selectedBlock ? this.structure.getBlock(this.selectedBlock) : null;
            if (block) {
                super.showSidePanel();
            }
        }
    }

    class FileInfoEditor extends TreeEditor {
        constructor(root, vscode, editHandler, readOnly) {
            super(root, vscode, editHandler, true);
        }
        onInit(file) {
            var _a;
            this.file.root.clear()
                .set('RootName', new NbtString(file.name))
                .set('Endianness', new NbtString(file.littleEndian ? 'little' : 'big'))
                .set('Compression', new NbtString((_a = file.compression) !== null && _a !== void 0 ? _a : 'none'));
            if (file.bedrockHeader) {
                this.file.root.set('BedrockHeader', new NbtInt(file.bedrockHeader));
            }
            super.onInit(this.file);
        }
        onUpdate(file) {
            this.onInit(file);
        }
        menu() {
            return [];
        }
    }

    class MapEditor {
        constructor(root, vscode, editHandler, readOnly) {
            this.root = root;
            this.vscode = vscode;
            this.editHandler = editHandler;
            this.readOnly = readOnly;
            this.file = NbtFile.create();
        }
        reveal() {
            const content = document.createElement('div');
            content.classList.add('nbt-content');
            const canvas = document.createElement('canvas');
            canvas.classList.add('nbt-map');
            canvas.width = 128;
            canvas.height = 128;
            const ctx = canvas.getContext('2d');
            this.paint(ctx);
            content.append(canvas);
            this.root.append(content);
        }
        onInit(file) {
            this.file = file;
        }
        onUpdate(file, edit) {
            this.onInit(file);
        }
        paint(ctx) {
            var _a, _b, _c;
            const img = ctx.createImageData(128, 128);
            const colors = this.file.root.getCompound('data').getByteArray('colors');
            for (let x = 0; x < 128; x += 1) {
                for (let z = 0; z < 128; z += 1) {
                    const id = (((_a = colors[x + z * 128]) !== null && _a !== void 0 ? _a : 0) + 256) % 256;
                    const base = (_b = colorIds[id >> 2]) !== null && _b !== void 0 ? _b : [0, 0, 0];
                    const m = (_c = multipliers[id & 0b11]) !== null && _c !== void 0 ? _c : 1;
                    const color = [base[0] * m, base[1] * m, base[2] * m];
                    const i = x * 4 + z * 4 * 128;
                    img.data[i] = color[0];
                    img.data[i + 1] = color[1];
                    img.data[i + 2] = color[2];
                    img.data[i + 3] = id < 4 ? 0 : 255;
                }
            }
            ctx.putImageData(img, 0, 0);
        }
    }
    const multipliers = [
        0.71,
        0.86,
        1,
        0.53,
    ];
    const colorIds = [
        [0, 0, 0],
        [127, 178, 56],
        [247, 233, 163],
        [199, 199, 199],
        [255, 0, 0],
        [160, 160, 255],
        [167, 167, 167],
        [0, 124, 0],
        [255, 255, 255],
        [164, 168, 184],
        [151, 109, 77],
        [112, 112, 112],
        [64, 64, 255],
        [143, 119, 72],
        [255, 252, 245],
        [216, 127, 51],
        [178, 76, 216],
        [102, 153, 216],
        [229, 229, 51],
        [127, 204, 25],
        [242, 127, 165],
        [76, 76, 76],
        [153, 153, 153],
        [76, 127, 153],
        [127, 63, 178],
        [51, 76, 178],
        [102, 76, 51],
        [102, 127, 51],
        [153, 51, 51],
        [25, 25, 25],
        [250, 238, 77],
        [92, 219, 213],
        [74, 128, 255],
        [0, 217, 58],
        [129, 86, 49],
        [112, 2, 0],
        [209, 177, 161],
        [159, 82, 36],
        [149, 87, 108],
        [112, 108, 138],
        [186, 133, 36],
        [103, 117, 53],
        [160, 77, 78],
        [57, 41, 35],
        [135, 107, 98],
        [87, 92, 92],
        [122, 73, 88],
        [76, 62, 92],
        [76, 50, 35],
        [76, 82, 42],
        [142, 60, 46],
        [37, 22, 16],
        [189, 48, 49],
        [148, 63, 97],
        [92, 25, 29],
        [22, 126, 134],
        [58, 142, 140],
        [86, 44, 62],
        [20, 180, 133],
        [100, 100, 100],
        [216, 175, 147],
        [127, 167, 150],
    ];

    class SnbtEditor {
        constructor(root, vscode, editHandler, readOnly) {
            this.root = root;
            this.vscode = vscode;
            this.editHandler = editHandler;
            this.readOnly = readOnly;
            this.snbt = '';
        }
        reveal() {
            var _a, _b;
            const content = document.createElement('div');
            content.classList.add('nbt-content');
            const textarea = document.createElement('textarea');
            textarea.classList.add('snbt-editor');
            textarea.textContent = this.snbt;
            textarea.rows = ((_b = (_a = this.snbt.match(/\n/g)) === null || _a === void 0 ? void 0 : _a.length) !== null && _b !== void 0 ? _b : 0) + 1;
            textarea.addEventListener('change', () => {
                const newRoot = NbtTag.fromString(textarea.value);
                this.editHandler({ type: 'set', path: [], old: this.file.root.toJsonWithId(), new: newRoot.toJsonWithId() });
            });
            content.append(textarea);
            this.root.append(content);
        }
        onInit(file) {
            this.file = file;
            this.snbt = file.root.toPrettyString('  ');
            const textarea = this.root.querySelector('.snbt-editor');
            if (textarea) {
                textarea.textContent = this.snbt;
            }
        }
        onUpdate(file) {
            this.onInit(file);
        }
        menu() {
            const copyButton = document.createElement('div');
            copyButton.classList.add('btn');
            copyButton.textContent = locale('copy');
            copyButton.addEventListener('click', () => {
                const textarea = this.root.querySelector('.snbt-editor');
                textarea.select();
                document.execCommand('copy');
            });
            return [copyButton];
        }
    }

    const TYPES = Object.keys(NbtType).filter(e => isNaN(Number(e)));
    const vscode = acquireVsCodeApi();
    const root = document.querySelector('.nbt-editor');
    function lazy(getter) {
        let value = null;
        return () => {
            if (value === null) {
                value = getter();
            }
            return value;
        };
    }
    class Editor {
        constructor() {
            var _a, _b, _c, _d, _e, _f, _g, _h, _j;
            this.panels = {
                structure: {
                    editor: lazy(() => new StructureEditor(root, vscode, e => this.makeEdit(e), this.readOnly)),
                    options: ['structure', 'default', 'snbt', 'info'],
                },
                map: {
                    editor: lazy(() => new MapEditor(root, vscode, e => this.makeEdit(e), this.readOnly)),
                    options: ['map', 'default', 'snbt', 'info'],
                },
                chunk: {
                    editor: lazy(() => new ChunkEditor(root, vscode, e => this.makeEdit(e), this.readOnly)),
                    options: ['chunk', 'default', 'snbt'],
                },
                default: {
                    editor: lazy(() => new TreeEditor(root, vscode, e => this.makeEdit(e), this.readOnly)),
                    options: ['default', 'snbt', 'info'],
                },
                snbt: {
                    editor: lazy(() => new SnbtEditor(root, vscode, e => this.makeEdit(e), this.readOnly)),
                },
                info: {
                    editor: lazy(() => new FileInfoEditor(root, vscode, () => { }, this.readOnly)),
                },
            };
            this.searchQuery = {};
            this.searchResults = null;
            this.searchIndex = 0;
            this.lastReplace = null;
            this.inMap = false;
            this.selectedChunk = { x: 0, z: 0 };
            this.invalidChunks = new Set();
            this.messageCallbacks = new Map();
            this.messageId = 1;
            window.addEventListener('message', async (e) => {
                this.onMessage(e.data);
            });
            this.findWidget = document.querySelector('.find-widget');
            const fw = this.findWidget;
            const findTypeSelect = fw === null || fw === void 0 ? void 0 : fw.querySelector('.find-part > .type-select > select');
            const findNameInput = fw === null || fw === void 0 ? void 0 : fw.querySelector('.find-part > .name-input');
            const findValueInput = fw === null || fw === void 0 ? void 0 : fw.querySelector('.find-part > .value-input');
            if (findTypeSelect && findNameInput && findValueInput) {
                findTypeSelect.addEventListener('change', () => {
                    findTypeSelect.parentElement.setAttribute('data-icon', findTypeSelect.value);
                    this.doSearch();
                });
                fw.querySelectorAll('.type-select select').forEach(select => {
                    ['Any', ...TYPES].filter(e => e !== 'End').forEach(t => {
                        const option = document.createElement('option');
                        option.value = t;
                        option.textContent = t.charAt(0).toUpperCase() + t.slice(1).split(/(?=[A-Z])/).join(' ');
                        select.append(option);
                    });
                    select.parentElement.setAttribute('data-icon', 'Any');
                });
                (_a = fw.querySelector('.find-part')) === null || _a === void 0 ? void 0 : _a.addEventListener('keyup', evt => {
                    if (evt.key !== 'Enter') {
                        this.doSearch();
                    }
                });
                (_b = fw.querySelector('.find-part')) === null || _b === void 0 ? void 0 : _b.addEventListener('keydown', evt => {
                    if (evt.key === 'Enter') {
                        if (evt.shiftKey) {
                            this.showMatch(this.searchIndex - 1);
                        }
                        else {
                            this.showMatch(this.searchIndex + 1);
                        }
                    }
                });
                (_c = fw.querySelector('.previous-match')) === null || _c === void 0 ? void 0 : _c.addEventListener('click', () => {
                    this.showMatch(this.searchIndex - 1);
                });
                (_d = fw.querySelector('.next-match')) === null || _d === void 0 ? void 0 : _d.addEventListener('click', () => {
                    this.showMatch(this.searchIndex + 1);
                });
                (_e = fw.querySelector('.close-widget')) === null || _e === void 0 ? void 0 : _e.addEventListener('click', () => {
                    fw.classList.remove('visible');
                });
                (_f = fw.querySelector('.replace-part')) === null || _f === void 0 ? void 0 : _f.addEventListener('keydown', evt => {
                    if (evt.key === 'Enter') {
                        if (evt.altKey && evt.ctrlKey) {
                            this.doReplaceAll();
                        }
                        else {
                            this.doReplace();
                        }
                    }
                });
                const replaceExpand = fw.querySelector('.replace-expand');
                replaceExpand === null || replaceExpand === void 0 ? void 0 : replaceExpand.addEventListener('click', () => {
                    const expanded = fw.classList.toggle('expanded');
                    replaceExpand === null || replaceExpand === void 0 ? void 0 : replaceExpand.classList.remove('codicon-chevron-right', 'codicon-chevron-down');
                    replaceExpand === null || replaceExpand === void 0 ? void 0 : replaceExpand.classList.add(expanded ? 'codicon-chevron-down' : 'codicon-chevron-right');
                });
                (_g = fw.querySelector('.replace')) === null || _g === void 0 ? void 0 : _g.addEventListener('click', () => {
                    this.doReplace();
                });
                (_h = fw.querySelector('.replace-all')) === null || _h === void 0 ? void 0 : _h.addEventListener('click', () => {
                    this.doReplaceAll();
                });
            }
            (_j = document.querySelector('.region-menu .btn')) === null || _j === void 0 ? void 0 : _j.addEventListener('click', () => {
                this.inMap = !this.inMap;
                this.updateRegionMap();
            });
            document.querySelectorAll('.region-menu input').forEach(el => {
                el.addEventListener('change', () => {
                    this.refreshChunk();
                });
                el.addEventListener('keydown', evt => {
                    if (evt.key === 'Enter') {
                        this.refreshChunk();
                    }
                });
            });
            document.addEventListener('keydown', evt => {
                var _a, _b;
                if (evt.ctrlKey && (evt.code === 'KeyF' || evt.code === 'KeyH')) {
                    if (!findTypeSelect || !findNameInput || !findValueInput)
                        return;
                    const replaceExpand = fw === null || fw === void 0 ? void 0 : fw.querySelector('.replace-expand');
                    fw.classList.add('visible');
                    if (this.searchQuery.name) {
                        findNameInput.focus();
                        findNameInput.setSelectionRange(0, findNameInput.value.length);
                    }
                    else {
                        findValueInput.focus();
                        findValueInput.setSelectionRange(0, findValueInput.value.length);
                    }
                    fw.classList.toggle('expanded', evt.code === 'KeyH');
                    replaceExpand === null || replaceExpand === void 0 ? void 0 : replaceExpand.classList.remove('codicon-chevron-right', 'codicon-chevron-down');
                    replaceExpand === null || replaceExpand === void 0 ? void 0 : replaceExpand.classList.add(evt.code === 'KeyH' ? 'codicon-chevron-down' : 'codicon-chevron-right');
                    if (this.searchResults && this.searchResults.length > 0) {
                        this.searchResults[this.searchIndex].show();
                    }
                }
                else if (evt.key === 'Escape') {
                    this.findWidget.classList.remove('visible');
                    (_b = (_a = this.getPanel()) === null || _a === void 0 ? void 0 : _a.onSearch) === null || _b === void 0 ? void 0 : _b.call(_a, null);
                }
            });
            document.addEventListener('contextmenu', evt => {
                evt.preventDefault();
            });
            vscode.postMessage({ type: 'ready' });
        }
        onMessage(m) {
            var _a, _b, _c, _d, _e, _f;
            switch (m.type) {
                case 'init':
                    console.log(m.body);
                    this.type = m.body.type;
                    this.nbtFile = m.body.type === 'region'
                        ? NbtRegion.fromJson(m.body.content, (x, z) => this.getChunkData(x, z))
                        : NbtFile.fromJson(m.body.content);
                    console.log(this.nbtFile);
                    this.readOnly = m.body.readOnly;
                    if (this.nbtFile instanceof NbtRegion.Ref) {
                        this.type = 'chunk';
                        this.activePanel = 'default';
                        this.inMap = true;
                        this.invalidChunks.clear();
                        this.updateRegionMap();
                    }
                    else {
                        this.setPanel(this.type);
                    }
                    return;
                case 'update':
                    try {
                        applyEdit(this.nbtFile, m.body);
                        const { file: editedFile, edit } = getEditedFile(this.nbtFile, m.body);
                        if (editedFile) {
                            this.refreshSearch();
                            Object.values(this.panels).forEach(p => p.updated = false);
                            (_b = (_a = this.getPanel()) === null || _a === void 0 ? void 0 : _a.onUpdate) === null || _b === void 0 ? void 0 : _b.call(_a, editedFile, edit);
                            this.panels[this.activePanel].updated = true;
                        }
                    }
                    catch (e) {
                        vscode.postMessage({ type: 'error', body: e.message });
                    }
                    return;
                case 'response':
                    const callback = this.messageCallbacks.get((_c = m.requestId) !== null && _c !== void 0 ? _c : 0);
                    if (m.body) {
                        callback === null || callback === void 0 ? void 0 : callback.res(m.body);
                    }
                    else {
                        callback === null || callback === void 0 ? void 0 : callback.rej((_d = m.error) !== null && _d !== void 0 ? _d : 'Unknown response');
                    }
                    return;
                default:
                    (_f = (_e = this.panels[this.type].editor()).onMessage) === null || _f === void 0 ? void 0 : _f.call(_e, m);
            }
        }
        async sendMessageWithResponse(message) {
            const requestId = this.messageId++;
            const promise = new Promise((res, rej) => {
                this.messageCallbacks.set(requestId, { res, rej });
            });
            vscode.postMessage(Object.assign(Object.assign({}, message), { requestId }));
            return promise;
        }
        async getChunkData(x, z) {
            const data = await this.sendMessageWithResponse({ type: 'getChunkData', body: { x, z } });
            const chunk = NbtFile.fromJson(data);
            return chunk;
        }
        getPanel() {
            var _a;
            return (_a = this.panels[this.activePanel]) === null || _a === void 0 ? void 0 : _a.editor();
        }
        setPanel(panel) {
            var _a, _b;
            root.innerHTML = '<div class="spinner"></div>';
            (_b = (_a = this.getPanel()) === null || _a === void 0 ? void 0 : _a.hide) === null || _b === void 0 ? void 0 : _b.call(_a);
            this.activePanel = panel;
            const editorPanel = this.getPanel();
            this.setPanelMenu(editorPanel);
            setTimeout(async () => {
                var _a, _b, _c;
                if (!this.panels[panel].updated) {
                    try {
                        if (this.nbtFile instanceof NbtRegion.Ref) {
                            const chunk = this.nbtFile.findChunk(this.selectedChunk.x, this.selectedChunk.z);
                            const file = chunk === null || chunk === void 0 ? void 0 : chunk.getFile();
                            if (chunk && file) {
                                (_a = editorPanel.onInit) === null || _a === void 0 ? void 0 : _a.call(editorPanel, file);
                            }
                        }
                        else {
                            (_b = editorPanel.onInit) === null || _b === void 0 ? void 0 : _b.call(editorPanel, this.nbtFile);
                        }
                    }
                    catch (e) {
                        if (e instanceof Error) {
                            console.error(e);
                            const div = document.createElement('div');
                            div.classList.add('nbt-content', 'error');
                            div.textContent = e.message;
                            root.innerHTML = '';
                            root.append(div);
                            return;
                        }
                    }
                    this.panels[panel].updated = true;
                }
                root.innerHTML = '';
                (_c = editorPanel === null || editorPanel === void 0 ? void 0 : editorPanel.reveal) === null || _c === void 0 ? void 0 : _c.call(editorPanel);
            });
        }
        setPanelMenu(panel) {
            var _a, _b, _c;
            const el = document.querySelector('.panel-menu');
            el.innerHTML = '';
            const btnGroup = document.createElement('div');
            btnGroup.classList.add('btn-group');
            el.append(btnGroup);
            (_a = this.panels[this.type].options) === null || _a === void 0 ? void 0 : _a.forEach((p) => {
                const button = document.createElement('div');
                btnGroup.append(button);
                button.classList.add('btn');
                button.textContent = locale(`panel.${p}`);
                if (p === this.activePanel) {
                    button.classList.add('active');
                }
                else {
                    button.addEventListener('click', () => this.setPanel(p));
                }
            });
            const menuPanels = (_c = (_b = panel.menu) === null || _b === void 0 ? void 0 : _b.call(panel)) !== null && _c !== void 0 ? _c : [];
            if (menuPanels.length > 0) {
                el.insertAdjacentHTML('beforeend', '<div class="menu-spacer"></div>');
                menuPanels.forEach(e => el.append(e));
            }
        }
        updateRegionMap() {
            var _a, _b, _c, _d, _e;
            if (!(this.nbtFile instanceof NbtRegion.Ref)) {
                return;
            }
            (_a = document.querySelector('.region-menu .btn')) === null || _a === void 0 ? void 0 : _a.classList.toggle('active', this.inMap);
            (_b = document.querySelector('.panel-menu')) === null || _b === void 0 ? void 0 : _b.classList.toggle('hidden', this.inMap);
            (_c = document.querySelector('.nbt-editor')) === null || _c === void 0 ? void 0 : _c.classList.toggle('hidden', this.inMap);
            (_d = document.querySelector('.region-map')) === null || _d === void 0 ? void 0 : _d.remove();
            if (this.inMap) {
                const map = document.createElement('div');
                map.classList.add('region-map');
                for (let z = 0; z < 32; z += 1) {
                    for (let x = 0; x < 32; x += 1) {
                        const chunk = this.nbtFile.findChunk(x, z);
                        const cell = document.createElement('div');
                        cell.classList.add('region-map-chunk');
                        cell.textContent = `${x} ${z}`;
                        cell.classList.toggle('empty', chunk === undefined);
                        cell.classList.toggle('loaded', (_e = chunk === null || chunk === void 0 ? void 0 : chunk.isResolved()) !== null && _e !== void 0 ? _e : false);
                        cell.classList.toggle('invalid', this.invalidChunks.has(`${x} ${z}`));
                        if (chunk !== undefined) {
                            cell.addEventListener('click', () => {
                                this.selectChunk(x, z);
                            });
                        }
                        cell.setAttribute('data-pos', `${x} ${z}`);
                        map.append(cell);
                    }
                }
                document.body.append(map);
            }
        }
        refreshChunk() {
            var _a, _b;
            if (!(this.nbtFile instanceof NbtRegion.Ref)) {
                return;
            }
            const x = (_a = getInt(document.getElementById('chunk-x'))) !== null && _a !== void 0 ? _a : 0;
            const z = (_b = getInt(document.getElementById('chunk-z'))) !== null && _b !== void 0 ? _b : 0;
            this.selectChunk(x, z);
        }
        async selectChunk(x, z) {
            var _a, _b, _c;
            if (!(this.nbtFile instanceof NbtRegion.Ref)) {
                return;
            }
            x = Math.max(0, Math.min(31, Math.floor(x)));
            z = Math.max(0, Math.min(31, Math.floor(z)));
            document.getElementById('chunk-x').value = `${x}`;
            document.getElementById('chunk-z').value = `${z}`;
            if (this.selectedChunk.x === x && this.selectedChunk.z === z) {
                this.inMap = false;
                this.updateRegionMap();
                return;
            }
            this.selectedChunk = { x, z };
            const chunk = this.nbtFile.findChunk(x, z);
            if (!chunk) {
                this.invalidChunks.add(`${x} ${z}`);
                (_a = document.querySelector('.region-menu')) === null || _a === void 0 ? void 0 : _a.classList.add('invalid');
                this.updateRegionMap();
                return;
            }
            Object.values(this.panels).forEach(p => p.updated = false);
            try {
                await chunk.getFileAsync();
            }
            catch (e) {
                this.invalidChunks.add(`${x} ${z}`);
                (_b = document.querySelector('.region-menu')) === null || _b === void 0 ? void 0 : _b.classList.add('invalid');
                this.updateRegionMap();
                return;
            }
            (_c = document.querySelector('.region-menu')) === null || _c === void 0 ? void 0 : _c.classList.remove('invalid');
            this.setPanel(this.activePanel);
            this.inMap = false;
            this.updateRegionMap();
            this.setPanel(this.activePanel);
        }
        doSearch() {
            const findPart = this.findWidget === null || this.findWidget === void 0 ? void 0 : this.findWidget.querySelector('.find-part');
            if (!findPart)
                return;
            const query = this.getQuery(findPart);
            if (['type', 'name', 'value'].every(e => { var _a; return ((_a = this.searchQuery) === null || _a === void 0 ? void 0 : _a[e]) === query[e]; })) {
                return;
            }
            this.searchQuery = query;
            this.searchIndex = 0;
            this.refreshSearch();
        }
        refreshSearch() {
            var _a, _b, _c;
            const editorPanel = this.getPanel();
            if ((editorPanel === null || editorPanel === void 0 ? void 0 : editorPanel.onSearch) && (this.searchQuery.name || this.searchQuery.value || this.searchQuery.type)) {
                this.searchResults = editorPanel.onSearch(this.searchQuery);
            }
            else {
                this.searchResults = null;
            }
            if (this.lastReplace && ((_c = (_b = (_a = this.searchResults) === null || _a === void 0 ? void 0 : _a[this.searchIndex]) === null || _b === void 0 ? void 0 : _b.path) === null || _c === void 0 ? void 0 : _c.equals(this.lastReplace))) {
                this.searchIndex += 1;
                this.lastReplace = null;
            }
            this.showMatch(this.searchIndex);
        }
        doReplace() {
            if (this.searchResults === null || this.searchResults.length === 0)
                return;
            const query = this.getQuery(this.findWidget.querySelector('.replace-part'));
            if (query.name || query.value || query.type) {
                const result = this.searchResults[this.searchIndex];
                this.lastReplace = result.path;
                this.makeEdit(result.replace(query));
            }
            console.log('Done replace!');
        }
        doReplaceAll() {
            if (!this.searchResults)
                return;
            const query = this.getQuery(this.findWidget.querySelector('.replace-part'));
            if (query.name || query.value || query.type) {
                const edits = this.searchResults.map(r => r.replace(query));
                this.makeEdit({ type: 'composite', edits });
            }
        }
        getQuery(element) {
            if (!element) {
                return { type: undefined, name: undefined, value: undefined };
            }
            const typeEl = element.querySelector('.type-select > select');
            const typeQuery = typeEl === null || typeEl === void 0 ? void 0 : typeEl.value;
            const nameEl = element.querySelector('.name-input');
            const nameQuery = nameEl === null || nameEl === void 0 ? void 0 : nameEl.value;
            const valueEl = element.querySelector('.value-input');
            const valueQuery = valueEl === null || valueEl === void 0 ? void 0 : valueEl.value;
            return {
                type: !typeQuery || typeQuery === 'Any' ? undefined : TYPES.indexOf(typeQuery),
                name: nameQuery || undefined,
                value: valueQuery || undefined,
            };
        }
        showMatch(index) {
            var _a, _b;
            const matchesEl = this.findWidget === null || this.findWidget === void 0 ? void 0 : this.findWidget.querySelector('.matches');
            if (!matchesEl)
                return;
            if (this.searchResults === null || this.searchResults.length === 0) {
                matchesEl.textContent = 'No results';
                (_b = (_a = this.getPanel()) === null || _a === void 0 ? void 0 : _a.onSearch) === null || _b === void 0 ? void 0 : _b.call(_a, null);
            }
            else {
                const matches = this.searchResults.length;
                this.searchIndex = (index % matches + matches) % matches;
                matchesEl.textContent = `${this.searchIndex + 1} of ${matches}`;
                this.searchResults[this.searchIndex].show();
            }
            const fw = this.findWidget;
            if (fw) {
                fw.classList.toggle('no-results', this.searchResults !== null && this.searchResults.length === 0);
                fw.querySelectorAll('.previous-match, .next-match').forEach(e => e.classList.toggle('disabled', this.searchResults === null || this.searchResults.length === 0));
            }
        }
        makeEdit(edit) {
            if (this.readOnly)
                return;
            if (this.nbtFile instanceof NbtRegion.Ref) {
                edit = Object.assign(Object.assign({ type: 'chunk' }, this.selectedChunk), { edit });
            }
            console.warn('Edit', edit);
            vscode.postMessage({ type: 'edit', body: edit });
        }
    }
    window.lbaReadNbtFileToJson = (bytes) => {
        let littleEndian = false;
        try {
            let file = NbtFile.read(bytes, { littleEndian, bedrockHeader: littleEndian });
            if (!littleEndian && file.root.size === 0) {
                const bedrockFile = NbtFile.read(bytes, { littleEndian: true, bedrockHeader: true });
                if (bedrockFile.root.size > 0) {
                    file = bedrockFile;
                }
            }
            return file.toJson();
        }
        catch (_a) {
            littleEndian = !littleEndian;
            return NbtFile.read(bytes, { littleEndian, bedrockHeader: littleEndian }).toJson();
        }
    };
    window.lbaDispatchNbtViewerInit = (body) => {
        window.dispatchEvent(new MessageEvent('message', { data: { type: 'init', body } }));
    };
    if (typeof window.__lbaCameraFov !== 'number') {
        window.__lbaCameraFov = 70;
    }
    window.lbaSetCameraFov = (deg) => {
        const v = Math.max(0, Math.min(110, deg | 0));
        window.__lbaCameraFov = v;
        const ed = window.__lbaNbtEditor;
        if (!ed || !ed.panels) {
            return;
        }
        const key = ed.type === 'chunk' ? 'chunk' : ed.type === 'structure' ? 'structure' : null;
        if (!key) {
            return;
        }
        try {
            const se = ed.panels[key].editor();
            if (se && typeof se.render === 'function') {
                se.cFovDeg = v;
                se.render();
            }
        }
        catch (_a) {
        }
    };
    window.lbaSetNbtCameraDebug = (enabled) => {
        const v = !!enabled;
        window.__lbaNbtCameraDebug = v;
        const ed = window.__lbaNbtEditor;
        if (!ed || !ed.panels) {
            return;
        }
        const key = ed.type === 'chunk' ? 'chunk' : ed.type === 'structure' ? 'structure' : null;
        if (!key) {
            return;
        }
        try {
            const se = ed.panels[key].editor();
            if (se && typeof se.setCameraDebug === 'function') {
                se.setCameraDebug(v);
            }
        }
        catch (_a) {
        }
    };
    window.__lbaNbtEditor = new Editor();

    exports.TYPES = TYPES;

    Object.defineProperty(exports, '__esModule', { value: true });

    return exports;

})({});
//# sourceMappingURL=editor.js.map
