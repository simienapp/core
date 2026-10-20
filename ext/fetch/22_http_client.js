// Copyright 2018-2026 the Deno authors. MIT license.

(function () {
const { core, primordials } = __bootstrap;
const {
  op_fetch_custom_client,
  op_fetch_tls_key_null,
  op_fetch_tls_key_static,
} = core.ops;

// Upstream loaded this from deno_net (02_tls.js); simien_fetch carries the
// part createHttpClient needs: a PEM cert and key, or no client certificate.
function loadTlsKeyPair(api, { keyFormat, cert, key }) {
  if (keyFormat !== undefined && keyFormat !== "pem") {
    throw new TypeError(
      `If "keyFormat" is specified, it must be "pem": received "${keyFormat}"`,
    );
  }
  if (cert !== undefined && key === undefined) {
    throw new TypeError(
      `If \`cert\` is specified, \`key\` must be specified as well for \`${api}\``,
    );
  }
  if (cert === undefined && key !== undefined) {
    throw new TypeError(
      `If \`key\` is specified, \`cert\` must be specified as well for \`${api}\``,
    );
  }
  return cert !== undefined
    ? op_fetch_tls_key_static(cert, key)
    : op_fetch_tls_key_null();
}

const { internalRidSymbol } = core;
const {
  JSONStringify,
  ObjectAssign,
  ObjectDefineProperty,
  ObjectHasOwn,
  StringPrototypeStartsWith,
  SymbolDispose,
  TypeError,
} = primordials;

/**
 * @param {Deno.CreateHttpClientOptions} options
 * @returns {HttpClient}
 */
function createHttpClient(options) {
  // Don't mutate the caller's options object. Historically `caCerts` and
  // `proxy.transport` were written back onto whatever the user passed in,
  // which broke reuse of a single options object across multiple calls
  // (denoland/deno#29347).
  options = ObjectAssign({ __proto__: null }, options);
  options.caCerts = options.caCerts ?? [];
  if (options.proxy) {
    const proxy = ObjectAssign({ __proto__: null }, options.proxy);
    options.proxy = proxy;
    if (ObjectHasOwn(proxy, "transport")) {
      switch (proxy.transport) {
        case "http": {
          const url = proxy.url;
          if (
            StringPrototypeStartsWith(url, "https:") ||
            StringPrototypeStartsWith(url, "socks5:") ||
            StringPrototypeStartsWith(url, "socks5h:")
          ) {
            throw new TypeError(
              `The url passed into 'proxy.url' has an invalid scheme for this transport.`,
            );
          }
          proxy.transport = "http";
          break;
        }
        case "https": {
          const url = proxy.url;
          if (
            StringPrototypeStartsWith(url, "http:") ||
            StringPrototypeStartsWith(url, "socks5:") ||
            StringPrototypeStartsWith(url, "socks5h:")
          ) {
            throw new TypeError(
              `The url passed into 'proxy.url' has an invalid scheme for this transport.`,
            );
          }
          proxy.transport = "http";
          break;
        }
        case "socks5": {
          const url = proxy.url;
          if (
            !StringPrototypeStartsWith(url, "socks5:") &&
            !StringPrototypeStartsWith(url, "socks5h:")
          ) {
            throw new TypeError(
              `The url passed into 'proxy.url' has an invalid scheme for this transport.`,
            );
          }
          proxy.transport = "http";
          break;
        }
        case "tcp":
        case "unix":
        case "vsock": {
          break;
        }
        default: {
          throw new TypeError(
            `Invalid value for 'proxy.transport' option: ${
              JSONStringify(proxy.transport)
            }`,
          );
        }
      }
    } else {
      proxy.transport = "http";
    }
  }
  const keyPair = loadTlsKeyPair("Deno.createHttpClient", options);
  return new HttpClient(
    op_fetch_custom_client(
      options,
      keyPair,
    ),
  );
}

class HttpClient {
  #rid;

  /**
   * @param {number} rid
   */
  constructor(rid) {
    ObjectDefineProperty(this, internalRidSymbol, {
      __proto__: null,
      enumerable: false,
      value: rid,
    });
    this.#rid = rid;
  }

  close() {
    core.close(this.#rid);
  }

  [SymbolDispose]() {
    core.tryClose(this.#rid);
  }
}
const HttpClientPrototype = HttpClient.prototype;

return { createHttpClient, HttpClient, HttpClientPrototype };
})();
