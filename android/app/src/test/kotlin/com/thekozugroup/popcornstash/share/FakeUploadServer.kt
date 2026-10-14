package com.thekozugroup.popcornstash.share

import mockwebserver3.Dispatcher
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import mockwebserver3.RecordedRequest
import okio.Buffer
import java.util.concurrent.CopyOnWriteArrayList

/**
 * The resumable upload API as the server answers it (docs/architecture.md section 8,
 * uploads): create, PUT at an offset (409 with `received` on a mismatch), status, complete.
 */
class FakeUploadServer(private val chunkSize: Int = 5) : Dispatcher() {
    class Upload(val id: String, val filename: String, val size: Long, val body: Buffer = Buffer(), var state: String = "receiving")

    val uploads = linkedMapOf<String, Upload>()
    val requests = CopyOnWriteArrayList<RecordedRequest>()
    val createBodies = CopyOnWriteArrayList<String>()

    /** Answers for the next requests to a path, before the normal behaviour. */
    val overrides = mutableMapOf<String, ArrayDeque<MockResponse>>()

    private var next = 0

    fun override(method: String, path: String, response: MockResponse) {
        overrides.getOrPut("$method $path") { ArrayDeque() }.addLast(response)
    }

    private fun json(body: String, code: Int = 200) =
        MockResponse.Builder().code(code).addHeader("Content-Type", "application/json").body(body).build()

    @Synchronized
    override fun dispatch(request: RecordedRequest): MockResponse {
        requests += request
        val path = request.url.encodedPath
        overrides["${request.method} $path"]?.removeFirstOrNull()?.let { return it }
        val segments = path.removePrefix("/api/uploads").trim('/').split('/').filter { it.isNotEmpty() }
        return when {
            request.method == "POST" && path == "/api/uploads" -> {
                val text = request.body!!.utf8()
                createBodies += text
                val size = Regex("\"size\":(\\d+)").find(text)!!.groupValues[1].toLong()
                val name = Regex("\"filename\":\"([^\"]*)\"").find(text)!!.groupValues[1]
                val id = "u${++next}"
                uploads[id] = Upload(id, name, size)
                json("""{"id":"$id","chunk_size":$chunkSize,"received":0}""")
            }
            request.method == "PUT" && segments.size == 1 -> {
                val upload = uploads[segments[0]] ?: return json("""{"error":{"code":"not_found","message":"That upload does not exist."}}""", 404)
                val offset = request.url.queryParameter("offset")!!.toLong()
                if (offset != upload.body.size) {
                    return json(
                        """{"error":{"code":"conflict","message":"The upload is at another offset.","received":${upload.body.size}},"received":${upload.body.size}}""",
                        409,
                    )
                }
                upload.body.write(request.body!!.toByteArray())
                json("""{"received":${upload.body.size}}""")
            }
            request.method == "GET" && segments.size == 1 -> {
                val upload = uploads[segments[0]] ?: return json("""{"error":{"code":"not_found","message":"That upload does not exist."}}""", 404)
                json("""{"id":"${upload.id}","filename":"${upload.filename}","size":${upload.size},"received":${upload.body.size},"state":"${upload.state}"}""")
            }
            request.method == "POST" && segments.size == 2 && segments[1] == "complete" -> {
                val upload = uploads[segments[0]] ?: return json("""{"error":{"code":"not_found","message":"That upload does not exist."}}""", 404)
                if (upload.body.size != upload.size) {
                    return json("""{"error":{"code":"conflict","message":"Not all of it arrived."},"received":${upload.body.size}}""", 409)
                }
                upload.state = "complete"
                json(
                    """{"id":"d-${upload.id}","kind":"upload","name":"${upload.filename}","state":"completed","progress":1.0,
                    "total_bytes":${upload.size},"done_bytes":${upload.size},"down_bps":0,"up_bps":0,"created_at":1,"item_ids":[],"destinations":[]}""",
                )
            }
            request.method == "DELETE" && segments.size == 1 -> {
                uploads.remove(segments[0])
                MockResponse.Builder().code(204).build()
            }
            else -> MockResponse.Builder().code(404).build()
        }
    }

    fun install(server: MockWebServer) {
        server.dispatcher = this
    }
}

/** A shared file's bytes in memory. */
class BytesSource(private val bytes: ByteArray) : ChunkSource {
    var reads = 0
    var closed = false

    override fun read(offset: Long, length: Int): ByteArray {
        reads++
        val start = offset.toInt().coerceAtMost(bytes.size)
        return bytes.copyOfRange(start, (start + length).coerceAtMost(bytes.size))
    }

    override fun close() {
        closed = true
    }
}
