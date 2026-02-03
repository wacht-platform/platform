# Wacht Platform API

Backend API for the Wacht platform dashboard. Wacht is a development toolkit that helps you build enterprise apps fast - think authentication, user management, organizations, AI tools, and analytics all built-in.

This API handles all the admin stuff - creating deployments, managing users, setting up organizations, configuring AI agents, and basically everything you need to run your customer-facing applications.

## What's included

**Authentication stuff**
- User signup/signin with sessions
- MFA support
- Social logins (OAuth)
- Custom JWT templates
- User profiles with image uploads

**B2B features**
- Organizations and workspaces
- Custom roles and permissions
- Member invitations and management

**AI tools**
- AI agents with knowledge base integration
- Knowledge base management (supports PDFs)
- REST API tools and platform events

**Analytics**
- Real-time user metrics
- ClickHouse for fast analytics
- Dashboard stats and insights

**Infrastructure**
- Custom domain verification
- Email via Postmark
- File uploads and CDN
- Health checks

## Tech stack

- Rust (2024 edition) with Axum
- PostgreSQL for main data
- ClickHouse for analytics
- Qdrant for vector search
- Postmark for emails
- Tokio for async

## Getting started

You'll need:
- Rust 1.70+
- PostgreSQL 14+
- ClickHouse
- Qdrant

## Setup

**1. Clone and setup**

```bash
git clone <repository-url>
cd wacht-dashboard-api
```

**2. Environment variables**

Create a `.env` file:

```env
# Database
DATABASE_URL=postgresql://username:password@localhost/wacht_dashboard

# ClickHouse (Analytics)
CLICKHOUSE_URL=http://localhost:8123
CLICKHOUSE_DATABASE=analytics

# Qdrant (Vector Database)
QDRANT_URL=http://localhost:6333

# Email (Postmark)
POSTMARK_API_TOKEN=your_postmark_token

# JWT
JWT_SECRET=your_jwt_secret

# CDN/File Upload
CDN_ENDPOINT=your_cdn_endpoint
CDN_ACCESS_KEY=your_access_key
CDN_SECRET_KEY=your_secret_key

# Application
RUST_LOG=info
```

**3. Database**

```bash
# Install diesel CLI if you don't have it
cargo install diesel_cli --no-default-features --features postgres

# Run migrations
diesel migration run
```

**4. Run it**

```bash
# Development
cargo run

# Production
cargo build --release
./target/release/platform-api
```

API runs on `http://localhost:3001`

## Testing

```bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run specific test
cargo test test_name
```

## 🤝 Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add some amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

## License

Licensed under PolyForm Shield License 1.0.0 - see [LICENSE.md](LICENSE.md) for details.

Copyright © 2025 Intellinesia Labs (https://intellinesia.com/)

**TL;DR**: You can use, modify, and distribute this for any purpose except building competing products. Read the full license for details.

## 🔗 Related Projects

- [Console Frontend](https://github.com/wacht-platform/console) - React frontend for this API

## 📞 Support

For support and questions, please open an issue in the GitHub repository.
