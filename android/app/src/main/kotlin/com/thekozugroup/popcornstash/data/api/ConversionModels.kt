package com.thekozugroup.popcornstash.data.api

import kotlinx.serialization.Serializable

@Serializable
data class ConversionPolicy(val enabled: Boolean = false, val codec: String = "av1", val quality: String = "balanced", val compute: String = "auto")
@Serializable
data class ConversionEncoder(val codec: String, val name: String, val label: String, val hardware: Boolean, val device: String? = null)
@Serializable
data class ConversionJob(val id: String, val itemId: String? = null, val title: String, val state: String, val progress: Double = 0.0, val encoder: String? = null, val message: String? = null, val savedBytes: Long = 0)
@Serializable
data class ConversionSavings(val videos: Long = 0, val originalBytes: Long = 0, val currentBytes: Long = 0, val savedBytes: Long = 0, val percent: Double = 0.0)
@Serializable
data class LibraryConversion(val policy: ConversionPolicy = ConversionPolicy(), val encoders: List<ConversionEncoder> = emptyList(), val pending: Int = 0, val jobs: List<ConversionJob> = emptyList(), val savings: ConversionSavings = ConversionSavings(), val retryable: Int = 0)
